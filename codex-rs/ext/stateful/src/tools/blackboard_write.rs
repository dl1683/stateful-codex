use std::collections::HashMap;
use std::collections::HashSet;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
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
use crate::events::CaptureOutcome;
use crate::events::KnowledgeCategory;
use crate::events::receipt_text;
use crate::services::ProjectIntelligenceServices;

use crate::rule_capture::RuleSource;
use crate::rule_capture::store_user_rule;
use crate::user_messages::UserMessageRegistry;
use crate::user_rules::MAX_RULE_BYTES;
use crate::user_rules::RuleStanding;
use crate::user_rules::is_task_limited;
use crate::user_rules::reports_speech;

use super::blackboard_evidence::EvidenceArguments;
use super::blackboard_evidence::evidence_schema;
use super::blackboard_evidence::resolve_evidence;
use super::blackboard_premises::PremiseArguments;
use super::blackboard_premises::premise_schema;
use super::blackboard_premises::resolve_premises;
use super::bounded_json_output;
use super::parse_arguments;
use super::preflight_receipts;
use super::receipt_error;
use super::stable_id;
use super::worst_identifier;
use super::worst_receipt_error;

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
    #[serde(default)]
    premises: Vec<PremiseArguments>,
    /// For kind instruction: the user's exact words for one rule.
    user_quote: Option<String>,
    /// For kind instruction: whether the rule outlives the current task.
    rule_scope: Option<RuleScope>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RuleScope {
    Standing,
    Task,
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

/// Validates and persists one record for the batch tool.
struct BlackboardRecorder {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
    user_messages: UserMessageRegistry,
}

impl BlackboardRecorder {
    fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
        user_messages: UserMessageRegistry,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
            projects,
            event_sink,
            user_messages,
        }
    }

    /// Stores a rule only as the user wrote it: the whole sentence of a recorded human
    /// message of this thread that contains `userQuote`. Earlier turns are not searched
    /// (their stored summaries carry no input origin). Task-limited directions and relayed
    /// advice are not stored, whatever scope the caller claims.
    async fn record_instruction(
        &self,
        receipt_turn_id: &str,
        user_quote: Option<String>,
        rule_scope: Option<RuleScope>,
    ) -> Result<BlackboardEntry, FunctionCallError> {
        let (Some(quote), Some(scope)) = (user_quote, rule_scope) else {
            return Err(respond(
                "kind instruction needs userQuote (the user's exact words for one rule) and ruleScope (standing or task)",
            ));
        };
        if matches!(scope, RuleScope::Task) {
            return Err(respond(
                "nothing written: a task-limited direction applies in this conversation only",
            ));
        }
        let (message, clause) = self
            .user_messages
            .find(&self.thread_id, &self.project_id, &quote)
            .ok_or_else(|| {
                respond(
                    "userQuote is not inside exactly one complete sentence of a user message recorded in this thread",
                )
            })?;
        if is_task_limited(&clause) {
            return Err(respond(
                "nothing written: the user limited that sentence to the current task",
            ));
        }
        if reports_speech(&clause) {
            return Err(respond(
                "nothing written: that sentence relays someone else's words, not the user's rule",
            ));
        }
        let (thread_id, turn_id) = (self.thread_id.clone(), message.turn_id);
        if clause.len() > MAX_RULE_BYTES {
            return Err(respond(format!(
                "the sentence holding userQuote exceeds {MAX_RULE_BYTES} bytes; quote a shorter complete rule"
            )));
        }
        store_user_rule(
            &self.services,
            self.event_sink.as_deref(),
            &self.project_id,
            RuleSource {
                thread_id: &thread_id,
                turn_id: &turn_id,
                receipt_turn_id,
            },
            &clause,
            RuleStanding::Standing,
        )
        .await
        .map(|captured| captured.entry)
        .map_err(respond)
    }

    async fn record(
        &self,
        arguments: RecordArguments,
        turn_id: &str,
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
            premises,
            user_quote,
            rule_scope,
        } = arguments;
        if verification == BlackboardVerification::UserConfirmed {
            return Err(FunctionCallError::RespondToModel(
                "userConfirmed is issued only from a host-observed user action and cannot be selected by the model"
                    .to_string(),
            ));
        }
        if kind == BlackboardKind::Instruction {
            return self
                .record_instruction(turn_id, user_quote, rule_scope)
                .await;
        }
        if user_quote.is_some() || rule_scope.is_some() {
            return Err(respond(
                "userQuote and ruleScope apply to kind instruction only",
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
        let premises =
            resolve_premises(&self.project_id, &self.services, project_roots, premises).await?;
        let node_id = match node_id {
            Some(node_id) => HierarchyNodeId::parse(node_id).map_err(respond)?,
            None => match inferred_node_id {
                Some(node_id) => node_id,
                None => self.project_node_id().await?,
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
                    premises,
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
            event_sink.emit(StatefulEvent::KnowledgeCaptured {
                project_id: entry.value.project_id.clone(),
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.to_string(),
                entry_id: entry.id.to_string(),
                revision: entry.revision,
                category: match entry.value.kind {
                    BlackboardKind::Decision => KnowledgeCategory::Decision,
                    BlackboardKind::Fact if entry.value.content.starts_with("Recipe:") => {
                        KnowledgeCategory::Recipe
                    }
                    BlackboardKind::Instruction => KnowledgeCategory::Rule,
                    BlackboardKind::Fact
                    | BlackboardKind::Claim
                    | BlackboardKind::Number
                    | BlackboardKind::Strategy
                    | BlackboardKind::Question
                    | BlackboardKind::Contradiction
                    | BlackboardKind::Failure
                    | BlackboardKind::RejectedApproach
                    | BlackboardKind::Signal
                    | BlackboardKind::Note => KnowledgeCategory::Finding,
                },
                outcome: CaptureOutcome::Stored,
                text: receipt_text(&entry.value.content),
            });
        }
        Ok(entry)
    }

    async fn project_node_id(&self) -> Result<HierarchyNodeId, FunctionCallError> {
        self.services
            .project_node_id(&self.project_id)
            .await
            .map_err(FunctionCallError::RespondToModel)
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

pub(super) struct BlackboardBatchRecordTool {
    recorder: BlackboardRecorder,
    relator: BlackboardRelateTool,
}

impl BlackboardBatchRecordTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
        user_messages: UserMessageRegistry,
    ) -> Self {
        Self {
            recorder: BlackboardRecorder::new(
                project_id.clone(),
                thread_id,
                services.clone(),
                projects,
                event_sink.clone(),
                user_messages,
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
        preflight_receipts(
            &call,
            &json!({
                "recorded": u64::MAX,
                "failed": u64::MAX,
                "results": (0..records.len())
                    .map(|index| json!({
                        "index": index,
                        "entryId": worst_identifier(""),
                        "revision": u64::MAX,
                        "recorded": false,
                        "error": worst_receipt_error(),
                    }))
                    .collect::<Vec<_>>(),
                "relationsRecorded": u64::MAX,
                "relationsFailed": u64::MAX,
                "relationResults": relations
                    .iter()
                    .enumerate()
                    .map(|(index, relation)| json!({
                        "index": index,
                        "idempotencyKey": relation.idempotency_key,
                        "relationId": worst_identifier(""),
                        "revision": u64::MAX,
                        "recorded": false,
                        "error": worst_receipt_error(),
                    }))
                    .collect::<Vec<_>>(),
            }),
        )?;
        let mut results = Vec::with_capacity(records.len());
        let mut entry_ids = HashMap::with_capacity(records.len());
        let mut recorded = 0usize;
        let project_roots = if records
            .iter()
            .all(|record| record.evidence.is_empty() && record.premises.is_empty())
        {
            Vec::new()
        } else {
            self.recorder.project_roots().await?
        };
        for (index, record) in records.into_iter().enumerate() {
            let record_key = record.idempotency_key.clone();
            match self
                .recorder
                .record(record, &call.turn_id, &call.call_id, &project_roots)
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
                    "error": receipt_error(error),
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
                    "error": receipt_error(format!(
                        "fromRecordKey was not recorded successfully: {}",
                        relation.from_record_key
                    )),
                }));
                continue;
            };
            let Some(to_entry_id) = entry_ids.get(&relation.to_record_key) else {
                relation_results.push(json!({
                    "index": index,
                    "idempotencyKey": relation_key,
                    "recorded": false,
                    "error": receipt_error(format!(
                        "toRecordKey was not recorded successfully: {}",
                        relation.to_record_key
                    )),
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
                    "error": receipt_error(error),
                })),
            }
        }
        let relations_failed = relation_results.len().saturating_sub(relations_recorded);
        bounded_json_output(
            &call,
            json!({
                "recorded": recorded,
                "failed": failed,
                "results": results,
                "relationsRecorded": relations_recorded,
                "relationsFailed": relations_failed,
                "relationResults": relation_results,
            }),
        )
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardBatchRecordTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(BATCH_RECORD_TOOL_NAME)
    }

    fn exposure(&self) -> ToolExposure {
        // Prose-bearing mutations stay out of nested code mode: model-written JS
        // string literals break on quotes inside long semantic fields.
        ToolExposure::DirectModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: BATCH_RECORD_TOOL_NAME.to_string(),
            description: format!(
                "Persist 1-{MAX_BATCH_RECORDS} findings (and up to {MAX_BATCH_RELATIONS} relations among them by idempotencyKey) once their results are in: user-approved decisions with reasons and verified recipes ('Recipe:' facts with exact commands), both promoted; exact numbers with scope; failures; rejected approaches; open questions. The host stores rules the user marks as standing; record another user rule as kind instruction with userQuote and ruleScope, one per record. sourceVerified needs evidence copied from evidence_read. Items are idempotent."
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
            "verification": {"type": "string", "enum": ["unverified", "sourceVerified", "disputed", "stale"], "description": "sourceVerified only with evidence receipts; userConfirmed is host-issued and unavailable here."},
            "importance": {"type": "string", "enum": ["critical", "high", "normal", "low"]},
            "rootPromotion": {"type": "string", "enum": ["notPromoted", "candidate", "promoted"]},
            "evidence": evidence_schema(),
            "premises": premise_schema(),
            "userQuote": {"type": "string"},
            "ruleScope": {"type": "string", "enum": ["standing", "task"]}
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
        bounded_json_output(
            &call,
            json!({
                "relationId": relation.id.to_string(),
                "revision": relation.revision,
                "recorded": true,
            }),
        )
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

    fn exposure(&self) -> ToolExposure {
        // Specialized: discoverable through tool search instead of riding in every request.
        ToolExposure::DeferredModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: RELATE_TOOL_NAME.to_string(),
            description: "Persist a relationship between two blackboard entries; contradicts means incompatible findings.".to_string(),
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
