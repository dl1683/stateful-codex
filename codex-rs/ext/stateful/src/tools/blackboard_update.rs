use std::collections::HashSet;
use std::sync::Arc;

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
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStructuredValue;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
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

const UPDATE_TOOL_NAME: &str = "blackboard_update_batch";
const MAX_MUTATIONS: usize = 24;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateArguments {
    mutations: Vec<MutationArguments>,
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum MutationArguments {
    SetRootPromotion {
        entry_id: String,
        expected_revision: u64,
        root_promotion: RootPromotion,
    },
    Revise {
        entry_id: String,
        expected_revision: u64,
        kind: Option<BlackboardKind>,
        content: Option<String>,
        structured_value: Option<BlackboardStructuredValue>,
        #[serde(default)]
        clear_structured_value: bool,
        confidence_basis_points: Option<u16>,
        verification: Option<BlackboardVerification>,
        importance: Option<BlackboardImportance>,
        root_promotion: Option<RootPromotion>,
        evidence: Option<Vec<EvidenceArguments>>,
    },
    Supersede {
        entry_id: String,
        expected_revision: u64,
        successor_entry_id: String,
    },
    Retire {
        entry_id: String,
        expected_revision: u64,
    },
}

impl MutationArguments {
    fn entry_id(&self) -> &str {
        match self {
            Self::SetRootPromotion { entry_id, .. }
            | Self::Revise { entry_id, .. }
            | Self::Supersede { entry_id, .. }
            | Self::Retire { entry_id, .. } => entry_id,
        }
    }

    fn action_name(&self) -> &'static str {
        match self {
            Self::SetRootPromotion { .. } => "setRootPromotion",
            Self::Revise { .. } => "revise",
            Self::Supersede { .. } => "supersede",
            Self::Retire { .. } => "retire",
        }
    }
}

pub(super) struct BlackboardUpdateTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl BlackboardUpdateTool {
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
        let UpdateArguments { mutations } = parse_arguments(&call)?;
        if mutations.is_empty() || mutations.len() > MAX_MUTATIONS {
            return Err(FunctionCallError::RespondToModel(format!(
                "mutations must contain 1-{MAX_MUTATIONS} items"
            )));
        }
        let mut entry_ids = HashSet::with_capacity(mutations.len());
        for mutation in &mutations {
            if !entry_ids.insert(mutation.entry_id()) {
                return Err(FunctionCallError::RespondToModel(format!(
                    "each entry may appear only once per batch: {}",
                    mutation.entry_id()
                )));
            }
        }
        let mut updated = 0usize;
        let mut results = Vec::with_capacity(mutations.len());
        let revises_evidence = mutations.iter().any(|mutation| {
            matches!(
                mutation,
                MutationArguments::Revise {
                    evidence: Some(_),
                    ..
                }
            )
        });
        let project_roots = if revises_evidence {
            self.projects
                .read_project(self.project_id.clone())
                .await
                .map_err(respond)?
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "selected project no longer exists".to_string(),
                    )
                })?
                .roots
                .into_iter()
                .map(|root| std::path::PathBuf::from(root.path))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for (index, mutation) in mutations.into_iter().enumerate() {
            let action = mutation.action_name();
            let entry_id = mutation.entry_id().to_string();
            match self
                .apply_mutation(mutation, &call.call_id, &project_roots)
                .await
            {
                Ok(entry) => {
                    updated += 1;
                    results.push(json!({
                        "index": index,
                        "action": action,
                        "entryId": entry.id.to_string(),
                        "revision": entry.revision,
                        "state": entry.state,
                        "rootPromotion": entry.value.root_promotion,
                        "updated": true,
                    }));
                }
                Err(error) => results.push(json!({
                    "index": index,
                    "action": action,
                    "entryId": entry_id,
                    "updated": false,
                    "error": error.to_string(),
                })),
            }
        }
        Ok(Box::new(JsonToolOutput::new(json!({
            "updated": updated,
            "failed": results.len().saturating_sub(updated),
            "results": results,
        }))))
    }

    async fn apply_mutation(
        &self,
        mutation: MutationArguments,
        source_id: &str,
        project_roots: &[std::path::PathBuf],
    ) -> Result<BlackboardEntry, FunctionCallError> {
        let id = BlackboardEntryId::parse(mutation.entry_id()).map_err(respond)?;
        let store = self.services.blackboard().await.map_err(respond)?;
        let current = store
            .get_entry(&self.project_id, &id)
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!("blackboard entry not found: {id}"))
            })?;
        let mut update = BlackboardEntryUpdate {
            expected_revision: current.revision,
            kind: current.value.kind,
            content: current.value.content,
            structured_value: current.value.structured_value,
            confidence: current.value.confidence,
            verification: current.value.verification,
            importance: current.value.importance,
            root_promotion: current.value.root_promotion,
            evidence: current.value.evidence,
            state: BlackboardEntryState::Active,
            superseded_by: None,
            provenance: BlackboardProvenance {
                kind: BlackboardProvenanceKind::Agent,
                source_id: source_id.to_string(),
            },
        };
        match mutation {
            MutationArguments::SetRootPromotion {
                expected_revision,
                root_promotion,
                ..
            } => {
                update.expected_revision = expected_revision;
                update.root_promotion = root_promotion;
            }
            MutationArguments::Revise {
                expected_revision,
                kind,
                content,
                structured_value,
                clear_structured_value,
                confidence_basis_points,
                verification,
                importance,
                root_promotion,
                evidence,
                ..
            } => {
                if structured_value.is_some() && clear_structured_value {
                    return Err(FunctionCallError::RespondToModel(
                        "structuredValue and clearStructuredValue cannot be used together"
                            .to_string(),
                    ));
                }
                if kind.is_none()
                    && content.is_none()
                    && structured_value.is_none()
                    && !clear_structured_value
                    && confidence_basis_points.is_none()
                    && verification.is_none()
                    && importance.is_none()
                    && root_promotion.is_none()
                    && evidence.is_none()
                {
                    return Err(FunctionCallError::RespondToModel(
                        "revise must change at least one field".to_string(),
                    ));
                }
                let source_meaning_changed = kind.is_some_and(|kind| kind != update.kind)
                    || content
                        .as_ref()
                        .is_some_and(|content| content != &update.content)
                    || structured_value
                        .as_ref()
                        .is_some_and(|value| Some(value) != update.structured_value.as_ref())
                    || (clear_structured_value && update.structured_value.is_some());
                let revised_verification = verification.unwrap_or(update.verification);
                if revised_verification == BlackboardVerification::SourceVerified
                    && source_meaning_changed
                    && evidence.is_none()
                {
                    return Err(FunctionCallError::RespondToModel(
                        "changing source-verified meaning requires fresh evidence_read receipts"
                            .to_string(),
                    ));
                }
                update.expected_revision = expected_revision;
                update.kind = kind.unwrap_or(update.kind);
                update.content = content.unwrap_or(update.content);
                if clear_structured_value {
                    update.structured_value = None;
                } else if structured_value.is_some() {
                    update.structured_value = structured_value;
                }
                if let Some(confidence) = confidence_basis_points {
                    update.confidence =
                        ConfidenceScore::from_basis_points(confidence).map_err(respond)?;
                }
                update.verification = revised_verification;
                update.importance = importance.unwrap_or(update.importance);
                update.root_promotion = root_promotion.unwrap_or(update.root_promotion);
                if let Some(evidence) = evidence {
                    update.evidence = resolve_evidence(
                        &self.project_id,
                        &self.thread_id,
                        &self.services,
                        project_roots,
                        evidence,
                    )
                    .await?
                    .0;
                }
            }
            MutationArguments::Supersede {
                expected_revision,
                successor_entry_id,
                ..
            } => {
                update.expected_revision = expected_revision;
                update.state = BlackboardEntryState::Superseded;
                update.superseded_by =
                    Some(BlackboardEntryId::parse(successor_entry_id).map_err(respond)?);
            }
            MutationArguments::Retire {
                expected_revision, ..
            } => {
                update.expected_revision = expected_revision;
                update.state = BlackboardEntryState::Tombstoned;
            }
        }
        let entry = store
            .update_entry(&self.project_id, &id, update)
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

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardUpdateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(UPDATE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: UPDATE_TOOL_NAME.to_string(),
            description: format!(
                "Apply 1-{MAX_MUTATIONS} revision-guarded lifecycle decisions to existing blackboard knowledge. Use setRootPromotion when a candidate has durable project-wide relevance; promotion does not make uncertain knowledge verified. Use revise when meaning, confidence, verification, importance, or evidence changes. Changing source-verified meaning requires fresh evidence_read receipts; metadata-only changes do not. Use supersede when a newer active entry replaces an older conclusion, and retire only for obsolete knowledge with no successor. Each entry may appear once and each result succeeds or fails independently."
            ),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&update_schema()).unwrap_or_else(|error| {
                unreachable!("invalid static blackboard update schema: {error}")
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

fn update_schema() -> serde_json::Value {
    let shared = json!({
        "entryId": {"type": "string"},
        "expectedRevision": {"type": "integer", "minimum": 1}
    });
    let mut revise_properties = shared.clone();
    revise_properties["kind"] = json!({"type": "string", "enum": ["instruction", "fact", "claim", "number", "decision", "strategy", "question", "contradiction", "failure", "rejectedApproach", "signal", "note"]});
    revise_properties["content"] = json!({"type": "string"});
    revise_properties["structuredValue"] = json!({"type": "object", "properties": {"value": {"type": "string"}, "unit": {"type": ["string", "null"]}}, "required": ["value"], "additionalProperties": false});
    revise_properties["clearStructuredValue"] = json!({"type": "boolean"});
    revise_properties["confidenceBasisPoints"] =
        json!({"type": "integer", "minimum": 0, "maximum": 10000});
    revise_properties["verification"] = json!({
        "type": "string",
        "enum": ["unverified", "sourceVerified", "userConfirmed", "disputed", "stale"],
        "description": "sourceVerified records source-linked model verification, not host proof of the entry's inference, scope, authority, completeness, or lack of supersession."
    });
    revise_properties["importance"] =
        json!({"type": "string", "enum": ["critical", "high", "normal", "low"]});
    revise_properties["rootPromotion"] =
        json!({"type": "string", "enum": ["notPromoted", "candidate", "promoted"]});
    revise_properties["evidence"] = evidence_schema();
    json!({
        "type": "object",
        "properties": {
            "mutations": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_MUTATIONS,
                "items": {
                    "oneOf": [
                        mutation_schema("setRootPromotion", shared.clone(), json!({
                            "rootPromotion": {"type": "string", "enum": ["notPromoted", "candidate", "promoted"]}
                        }), &["rootPromotion"]),
                        mutation_schema("revise", revise_properties, json!({}), &[]),
                        mutation_schema("supersede", shared.clone(), json!({
                            "successorEntryId": {"type": "string"}
                        }), &["successorEntryId"]),
                        mutation_schema("retire", shared, json!({}), &[])
                    ]
                }
            }
        },
        "required": ["mutations"],
        "additionalProperties": false
    })
}

fn mutation_schema(
    action: &str,
    mut properties: serde_json::Value,
    additional: serde_json::Value,
    additional_required: &[&str],
) -> serde_json::Value {
    properties["action"] = json!({"type": "string", "enum": [action]});
    if let (Some(properties), Some(additional)) =
        (properties.as_object_mut(), additional.as_object())
    {
        properties.extend(additional.clone());
    }
    let mut required = vec!["action", "entryId", "expectedRevision"];
    required.extend_from_slice(additional_required);
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "blackboard_update_tests.rs"]
mod tests;
