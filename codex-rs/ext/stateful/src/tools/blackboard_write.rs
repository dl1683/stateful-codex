use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardRelationId;
use codex_project_intelligence::BlackboardRelationKind;
use codex_project_intelligence::BlackboardStructuredValue;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewBlackboardRelation;
use codex_project_intelligence::RootPromotion;
use serde::Deserialize;
use serde_json::json;

use crate::BlackboardEntityKind;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::stable_id;

const RECORD_TOOL_NAME: &str = "blackboard_record";
const BATCH_RECORD_TOOL_NAME: &str = "blackboard_record_batch";
const RELATE_TOOL_NAME: &str = "blackboard_relate";
const MAX_BATCH_RECORDS: usize = 16;

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
    evidence: Vec<BlackboardEvidenceLink>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BatchRecordArguments {
    records: Vec<RecordArguments>,
}

pub(super) struct BlackboardRecordTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl BlackboardRecordTool {
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
        let arguments: RecordArguments = parse_arguments(&call)?;
        let entry = self.record(arguments, &call.call_id).await?;
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
    ) -> Result<BlackboardEntry, FunctionCallError> {
        let node_id = match arguments.node_id {
            Some(node_id) => HierarchyNodeId::parse(node_id).map_err(respond)?,
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
                            "project hierarchy is empty; refresh the context map first".to_string(),
                        )
                    })?
                    .id
            }
        };
        let id = BlackboardEntryId::parse(stable_id(
            "entry",
            &self.project_id,
            &arguments.idempotency_key,
        ))
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
                    kind: arguments.kind,
                    content: arguments.content,
                    structured_value: arguments.structured_value,
                    confidence: ConfidenceScore::from_basis_points(
                        arguments.confidence_basis_points,
                    )
                    .map_err(respond)?,
                    verification: arguments.verification,
                    importance: arguments.importance,
                    root_promotion: arguments.root_promotion,
                    evidence: arguments.evidence,
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
}

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardRecordTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(RECORD_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: RECORD_TOOL_NAME.to_string(),
            description: "Persist one new item of materially reusable project understanding after examining evidence. Prefer blackboard_record_batch when committing two or more coherent findings. Do not record routine progress, cheap-to-recompute inventories, or knowledge already represented adequately. sourceVerified requires current context-map evidence links; a shell or tool result alone is not an evidence link. Omit nodeId only for project-wide knowledge. Reuse idempotencyKey only for an identical retry.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&record_schema())
            .unwrap_or_else(|error| unreachable!("invalid static blackboard record schema: {error}")),
            output_schema: None,
        })
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
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
}

impl BlackboardBatchRecordTool {
    pub(super) fn new(
        project_id: String,
        services: ProjectIntelligenceServices,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
    ) -> Self {
        Self {
            recorder: BlackboardRecordTool::new(project_id, services, event_sink),
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: BatchRecordArguments = parse_arguments(&call)?;
        if arguments.records.is_empty() || arguments.records.len() > MAX_BATCH_RECORDS {
            return Err(FunctionCallError::RespondToModel(format!(
                "records must contain 1-{MAX_BATCH_RECORDS} items"
            )));
        }
        let mut results = Vec::with_capacity(arguments.records.len());
        let mut recorded = 0usize;
        for (index, record) in arguments.records.into_iter().enumerate() {
            match self.recorder.record(record, &call.call_id).await {
                Ok(entry) => {
                    recorded += 1;
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
        Ok(Box::new(JsonToolOutput::new(json!({
            "recorded": recorded,
            "failed": results.len().saturating_sub(recorded),
            "results": results,
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
                "Persist 1-{MAX_BATCH_RECORDS} coherent, materially reusable findings in one bounded call. Each item is independently idempotent and returns its own success or error, so do not retry successful items. Prefer this over separate blackboard_record calls after one evidence-review pass."
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

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
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
            "verification": {"type": "string", "enum": ["unverified", "sourceVerified", "userConfirmed", "disputed", "stale"], "description": "Use sourceVerified only with current context-map evidence links."},
            "importance": {"type": "string", "enum": ["critical", "high", "normal", "low"]},
            "rootPromotion": {"type": "string", "enum": ["notPromoted", "candidate", "promoted"]},
            "evidence": {"type": "array", "description": "Current context-map links supporting sourceVerified knowledge.", "items": {"type": "object", "properties": {"contextMapEntryId": {"type": "string"}, "sourceFingerprint": {"type": "string"}}, "required": ["contextMapEntryId", "sourceFingerprint"], "additionalProperties": false}}
        },
        "required": ["idempotencyKey", "kind", "content", "confidenceBasisPoints", "verification", "importance", "rootPromotion"],
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
                        source_id: call.call_id,
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
        Ok(Box::new(JsonToolOutput::new(json!({
            "relationId": relation.id.to_string(),
            "revision": relation.revision,
            "recorded": true,
        }))))
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

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
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
