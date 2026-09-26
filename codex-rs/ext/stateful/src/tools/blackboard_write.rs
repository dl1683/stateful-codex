use std::collections::HashMap;
use std::collections::HashSet;

use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardRelation;
use codex_project_intelligence::BlackboardRelationId;
use codex_project_intelligence::BlackboardRelationKind;
use codex_project_intelligence::BlackboardStructuredValue;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewBlackboardRelation;
use codex_project_intelligence::RootPromotion;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use crate::BlackboardEntityKind;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;

use super::blackboard_evidence::EvidenceArguments;
use super::blackboard_evidence::evidence_schema;
use super::blackboard_evidence::resolve_evidence;
use super::parse_arguments;
use super::stable_id;

const RECORD_TOOL_NAME: &str = "blackboard_record";
const BATCH_RECORD_TOOL_NAME: &str = "blackboard_record_batch";
const RELATE_TOOL_NAME: &str = "blackboard_relate";
const MAX_BATCH_RECORDS: usize = 24;
const MAX_BATCH_RELATIONS: usize = 48;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordArguments {
    idempotency_key: String,
    node_id: Option<String>,
    kind: BlackboardKind,
    content: String,
    structured_value: Option<BlackboardStructuredValue>,
    confidence_basis_points: u16,
    verification: BlackboardVerification,
    importance: BlackboardImportance,
    root_promotion: RootPromotion,
    #[serde(default)]
    evidence: Vec<EvidenceArguments>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BatchRelationArguments {
    idempotency_key: String,
    from_record_key: String,
    to_record_key: String,
    kind: BlackboardRelationKind,
    note: Option<String>,
    confidence_basis_points: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BatchRecordArguments {
    records: Vec<RecordArguments>,
    #[serde(default)]
    relations: Vec<BatchRelationArguments>,
}

pub(super) struct BlackboardRecordTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl BlackboardRecordTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
            projects,
            event_sink,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: RecordArguments = parse_arguments(&call)?;
        let project_roots = if arguments.evidence.is_empty() {
            Vec::new()
        } else {
            self.project_roots().await?
        };
        let entry = self
            .record(arguments, &call.call_id, &project_roots)
            .await?;
        Ok(Box::new(JsonToolOutput::new(json!({
            "entryId": entry.id.to_string(),
            "revision": entry.revision,
            "recorded": true,
        }))))
    }

    async fn record(
        &self,
        arguments: RecordArguments,
        source_id: &str,
        project_roots: &[std::path::PathBuf],
    ) -> Result<BlackboardEntry, FunctionCallError> {
        let RecordArguments {
            idempotency_key,
            node_id,
            kind,
            content,
            structured_value,
            confidence_basis_points,
            verification,
            importance,
            root_promotion,
            evidence,
        } = arguments;
        if verification == BlackboardVerification::UserConfirmed {
            return Err(FunctionCallError::RespondToModel(
                "userConfirmed is issued only from a host-observed user action and cannot be selected by the model"
                    .to_string(),
            ));
        }
        let (evidence, inferred_node_id) = resolve_evidence(
            &self.project_id,
            &self.thread_id,
            &self.services,
            project_roots,
            evidence,
        )
        .await?;
        let node_id = match node_id {
            Some(node_id) => HierarchyNodeId::parse(node_id).map_err(respond)?,
            None => match inferred_node_id {
                Some(node_id) => node_id,
                None => {
                    self.services
                        .hierarchy()
                        .await
                        .map_err(respond)?
                        .project_node(&self.project_id)
                        .await
                        .map_err(respond)?
                        .ok_or_else(|| {
                            FunctionCallError::RespondToModel(
                                "project hierarchy is empty; refresh the context map first"
                                    .to_string(),
                            )
                        })?
                        .id
                }
            },
        };
        let id = BlackboardEntryId::parse(stable_id("entry", &self.project_id, &idempotency_key))
            .map_err(respond)?;
        let entry = self
            .services
            .blackboard()
            .await
            .map_err(respond)?
            .create_entry(
                id,
                NewBlackboardEntry {
                    project_id: self.project_id.clone(),
                    node_id,
                    kind,
                    content,
                    structured_value,
                    confidence: ConfidenceScore::from_basis_points(confidence_basis_points)
                        .map_err(respond)?,
                    verification,
                    importance,
                    root_promotion,
                    evidence,
                    provenance: BlackboardProvenance {
                        kind: BlackboardProvenanceKind::Agent,
                        source_id: source_id.to_string(),
                    },
                },
            )
            .await
            .map_err(respond)?;
        if let Some(event_sink) = &self.event_sink {
            event_sink.emit(StatefulEvent::BlackboardUpdated {
                project_id: entry.value.project_id.clone(),
                entity_kind: BlackboardEntityKind::Entry,
                entity_id: entry.id.to_string(),
                revision: entry.revision,
            });
        }
        Ok(entry)
    }

    async fn project_roots(&self) -> Result<Vec<std::path::PathBuf>, FunctionCallError> {
        self.projects
            .read_project(self.project_id.clone())
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel("selected project no longer exists".to_string())
            })
            .map(|project| {
                project
                    .roots
                    .into_iter()
                    .map(|root| std::path::PathBuf::from(root.path))
                    .collect()
            })
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardRecordTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(RECORD_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: RECORD_TOOL_NAME.to_string(),
            description: "Persist one new item of materially reusable project understanding after examining evidence. Prefer blackboard_record_batch when committing two or more coherent findings. Preserve decision-changing contrasts, exact values, qualifiers, scope or authority boundaries, and supersession signals; do not compress an entry to only what supports the immediate answer. Do not record routine progress, cheap-to-recompute inventories, or knowledge already represented adequately. sourceVerified requires host-issued read receipts and records that the model reviewed those exact source bytes as support; it does not mean the host proved the inference. userConfirmed is host-issued from an explicit user action and is unavailable to this model tool. Copy each non-null blackboardEvidence object returned by evidence_read unchanged into evidence. A shell result or route locator alone is not evidence. When nodeId is omitted, single-source evidence is attached to that file automatically and cross-source knowledge remains project-wide. Reuse idempotencyKey only for an identical retry.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&record_schema())
            .unwrap_or_else(|error| unreachable!("invalid static blackboard record schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

pub(super) struct BlackboardBatchRecordTool {
    recorder: BlackboardRecordTool,
    relator: BlackboardRelateTool,
}

impl BlackboardBatchRecordTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
    ) -> Self {
        Self {
            recorder: BlackboardRecordTool::new(
                project_id.clone(),
                thread_id,
                services.clone(),
                projects,
                event_sink.clone(),
            ),
            relator: BlackboardRelateTool::new(project_id, services, event_sink),
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let BatchRecordArguments { records, relations } = parse_arguments(&call)?;
        if records.is_empty() || records.len() > MAX_BATCH_RECORDS {
            return Err(FunctionCallError::RespondToModel(format!(
                "records must contain 1-{MAX_BATCH_RECORDS} items"
            )));
        }
        if relations.len() > MAX_BATCH_RELATIONS {
            return Err(FunctionCallError::RespondToModel(format!(
                "relations must contain 0-{MAX_BATCH_RELATIONS} items"
            )));
        }
        let mut seen_record_keys = HashSet::with_capacity(records.len());
        for record in &records {
            if !seen_record_keys.insert(record.idempotency_key.clone()) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "duplicate record idempotencyKey: {}",
                    record.idempotency_key
                )));
            }
        }
        let mut results = Vec::with_capacity(records.len());
        let mut entry_ids = HashMap::with_capacity(records.len());
        let mut recorded = 0usize;
        let project_roots = if records.iter().all(|record| record.evidence.is_empty()) {
            Vec::new()
        } else {
            self.recorder.project_roots().await?
        };
        for (index, record) in records.into_iter().enumerate() {
            let record_key = record.idempotency_key.clone();
            match self
                .recorder
                .record(record, &call.call_id, &project_roots)
                .await
            {
                Ok(entry) => {
                    recorded += 1;
                    entry_ids.insert(record_key, entry.id.clone());
                    results.push(json!({
                        "index": index,
                        "entryId": entry.id.to_string(),
                        "revision": entry.revision,
                        "recorded": true,
                    }));
                }
                Err(error) => results.push(json!({
                    "index": index,
                    "recorded": false,
                    "error": error.to_string(),
                })),
            }
        }
        let failed = results.len().saturating_sub(recorded);
        let mut relation_results = Vec::with_capacity(relations.len());
        let mut relations_recorded = 0usize;
        for (index, relation) in relations.into_iter().enumerate() {
            let relation_key = relation.idempotency_key.clone();
            let Some(from_entry_id) = entry_ids.get(&relation.from_record_key) else {
                relation_results.push(json!({
                    "index": index,
                    "idempotencyKey": relation_key,
                    "recorded": false,
                    "error": format!(
                        "fromRecordKey was not recorded successfully: {}",
                        relation.from_record_key
                    ),
                }));
                continue;
            };
            let Some(to_entry_id) = entry_ids.get(&relation.to_record_key) else {
                relation_results.push(json!({
                    "index": index,
                    "idempotencyKey": relation_key,
                    "recorded": false,
                    "error": format!(
                        "toRecordKey was not recorded successfully: {}",
                        relation.to_record_key
                    ),
                }));
                continue;
            };
            let arguments = RelateArguments {
                idempotency_key: relation.idempotency_key,
                from_entry_id: from_entry_id.to_string(),
                to_entry_id: to_entry_id.to_string(),
                kind: relation.kind,
                note: relation.note,
                confidence_basis_points: relation.confidence_basis_points,
            };
            match self.relator.relate(arguments, &call.call_id).await {
                Ok(created) => {
                    relations_recorded += 1;
                    relation_results.push(json!({
                        "index": index,
                        "idempotencyKey": relation_key,
                        "relationId": created.id.to_string(),
                        "revision": created.revision,
                        "recorded": true,
                    }));
                }
                Err(error) => relation_results.push(json!({
                    "index": index,
                    "idempotencyKey": relation_key,
                    "recorded": false,
                    "error": error.to_string(),
                })),
            }
        }
        let relations_failed = relation_results.len().saturating_sub(relations_recorded);
        Ok(Box::new(JsonToolOutput::new(json!({
            "recorded": recorded,
            "failed": failed,
            "results": results,
            "relationsRecorded": relations_recorded,
            "relationsFailed": relations_failed,
            "relationResults": relation_results,
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardBatchRecordTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(BATCH_RECORD_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: BATCH_RECORD_TOOL_NAME.to_string(),
            description: format!(
                "Persist 1-{MAX_BATCH_RECORDS} coherent, materially reusable findings and up to {MAX_BATCH_RELATIONS} relationships in one bounded call. Preserve decision-changing contrasts, exact values, qualifiers, scope or authority boundaries, and supersession signals instead of compressing the batch to the immediate answer. Relations reference record idempotencyKey values from this same call through fromRecordKey and toRecordKey, avoiding opaque entry-ID copying. Each item is independently idempotent and returns its own success or error, so do not retry successful items. Prefer this after one evidence-review pass."
            ),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "records": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_BATCH_RECORDS,
                        "items": record_schema()
                    },
                    "relations": {
                        "type": "array",
                        "maxItems": MAX_BATCH_RELATIONS,
                        "description": "Optional relationships among records in this call, referenced by record idempotencyKey.",
                        "items": batch_relation_schema()
                    }
                },
                "required": ["records"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| {
                unreachable!("invalid static blackboard batch schema: {error}")
            }),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

fn record_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "idempotencyKey": {"type": "string"},
            "nodeId": {"type": "string"},
            "kind": {"type": "string", "enum": ["instruction", "fact", "claim", "number", "decision", "strategy", "question", "contradiction", "failure", "rejectedApproach", "signal", "note"]},
            "content": {"type": "string"},
            "structuredValue": {"type": "object", "properties": {"value": {"type": "string"}, "unit": {"type": ["string", "null"]}}, "required": ["value"], "additionalProperties": false},
            "confidenceBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000},
            "verification": {"type": "string", "enum": ["unverified", "sourceVerified", "disputed", "stale"], "description": "Use sourceVerified only with current context-map evidence links. It records source-linked model verification, not host proof of the entry's inference, scope, authority, completeness, or lack of supersession. userConfirmed is host-issued from an explicit user action and is unavailable to this model tool."},
            "importance": {"type": "string", "enum": ["critical", "high", "normal", "low"]},
            "rootPromotion": {"type": "string", "enum": ["notPromoted", "candidate", "promoted"]},
            "evidence": evidence_schema()
        },
        "required": ["idempotencyKey", "kind", "content", "confidenceBasisPoints", "verification", "importance", "rootPromotion"],
        "additionalProperties": false
    })
}

fn batch_relation_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "idempotencyKey": {"type": "string"},
            "fromRecordKey": {"type": "string", "description": "idempotencyKey of the source record in this batch."},
            "toRecordKey": {"type": "string", "description": "idempotencyKey of the target record in this batch."},
            "kind": {"type": "string", "enum": ["supports", "contradicts", "dependsOn", "relatedTo"]},
            "note": {"type": "string"},
            "confidenceBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}
        },
        "required": ["idempotencyKey", "fromRecordKey", "toRecordKey", "kind", "confidenceBasisPoints"],
        "additionalProperties": false
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RelateArguments {
    idempotency_key: String,
    from_entry_id: String,
    to_entry_id: String,
    kind: BlackboardRelationKind,
    note: Option<String>,
    confidence_basis_points: u16,
}

pub(super) struct BlackboardRelateTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl BlackboardRelateTool {
    pub(super) fn new(
        project_id: String,
        services: ProjectIntelligenceServices,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
    ) -> Self {
        Self {
            project_id,
            services,
            event_sink,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: RelateArguments = parse_arguments(&call)?;
        let relation = self.relate(arguments, &call.call_id).await?;
        Ok(Box::new(JsonToolOutput::new(json!({
            "relationId": relation.id.to_string(),
            "revision": relation.revision,
            "recorded": true,
        }))))
    }

    async fn relate(
        &self,
        arguments: RelateArguments,
        source_id: &str,
    ) -> Result<BlackboardRelation, FunctionCallError> {
        let id = BlackboardRelationId::parse(stable_id(
            "relation",
            &self.project_id,
            &arguments.idempotency_key,
        ))
        .map_err(respond)?;
        let relation = self
            .services
            .blackboard()
            .await
            .map_err(respond)?
            .create_relation(
                id,
                NewBlackboardRelation {
                    project_id: self.project_id.clone(),
                    from_entry_id: BlackboardEntryId::parse(arguments.from_entry_id)
                        .map_err(respond)?,
                    to_entry_id: BlackboardEntryId::parse(arguments.to_entry_id)
                        .map_err(respond)?,
                    kind: arguments.kind,
                    note: arguments.note,
                    confidence: ConfidenceScore::from_basis_points(
                        arguments.confidence_basis_points,
                    )
                    .map_err(respond)?,
                    provenance: BlackboardProvenance {
                        kind: BlackboardProvenanceKind::Agent,
                        source_id: source_id.to_string(),
                    },
                },
            )
            .await
            .map_err(respond)?;
        if let Some(event_sink) = &self.event_sink {
            event_sink.emit(StatefulEvent::BlackboardUpdated {
                project_id: relation.value.project_id.clone(),
                entity_kind: BlackboardEntityKind::Relation,
                entity_id: relation.id.to_string(),
                revision: relation.revision,
            });
        }
        Ok(relation)
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardRelateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(RELATE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: RELATE_TOOL_NAME.to_string(),
            description: "Persist a meaningful relationship between two blackboard entries. Use contradictions for genuinely incompatible findings, not mere differences.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "idempotencyKey": {"type": "string"},
                    "fromEntryId": {"type": "string"},
                    "toEntryId": {"type": "string"},
                    "kind": {"type": "string", "enum": ["supports", "contradicts", "dependsOn", "relatedTo"]},
                    "note": {"type": "string"},
                    "confidenceBasisPoints": {"type": "integer", "minimum": 0, "maximum": 10000}
                },
                "required": ["idempotencyKey", "fromEntryId", "toEntryId", "kind", "confidenceBasisPoints"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static blackboard relate schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}
use std::sync::Arc;
