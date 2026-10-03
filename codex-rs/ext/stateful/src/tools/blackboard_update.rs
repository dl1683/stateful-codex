use std::collections::HashSet;
use std::sync::Arc;

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
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::BlackboardStructuredValue;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
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
use super::blackboard_premises::PremiseArguments;
use super::blackboard_premises::premise_schema;
use super::blackboard_premises::resolve_premises;
use super::bounded_json_output;
use super::parse_arguments;
use super::preflight_receipts;
use super::receipt_error;
use super::worst_identifier;
use super::worst_receipt_error;

const UPDATE_TOOL_NAME: &str = "blackboard_update_batch";
const MAX_MUTATIONS: usize = 24;
/// Host-written disclosure attached to every successful retire or supersede result.
///
/// Retiring only hides an entry from active channels; it is not a forget or delete
/// capability, and the model must not report it as one.
const RETENTION_DISCLOSURE: &str = "Hidden from active blackboard search and the root only. This is not forgetting or deletion: prior revisions, historical search, run goals and results, obligation packets, and prior-run outcomes injected into later sessions may still contain this content. Do not tell the user it was forgotten, removed, or deleted; tell them what still remains.";

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
        premises: Option<Vec<PremiseArguments>>,
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

    fn expected_revision(&self) -> u64 {
        match self {
            Self::SetRootPromotion {
                expected_revision, ..
            }
            | Self::Revise {
                expected_revision, ..
            }
            | Self::Supersede {
                expected_revision, ..
            }
            | Self::Retire {
                expected_revision, ..
            } => *expected_revision,
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
        preflight_receipts(
            &call,
            &json!({
                "updated": u64::MAX,
                "failed": u64::MAX,
                "results": mutations
                    .iter()
                    .enumerate()
                    .map(|(index, mutation)| {
                        let entry_id = worst_identifier(mutation.entry_id());
                        json!({
                            "index": index,
                            "action": "setRootPromotion",
                            "entryId": entry_id,
                            "revision": u64::MAX,
                            "state": "x".repeat(16),
                            "rootPromotion": "x".repeat(16),
                            "updated": false,
                            "historicalFinding": {"entryId": entry_id, "revision": u64::MAX},
                            "retention": RETENTION_DISCLOSURE,
                            "error": worst_receipt_error(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }),
        )?;
        let mut updated = 0usize;
        let mut results = Vec::with_capacity(mutations.len());
        let revises_support = mutations.iter().any(|mutation| {
            matches!(
                mutation,
                MutationArguments::Revise {
                    evidence: Some(_),
                    ..
                } | MutationArguments::Revise {
                    premises: Some(_),
                    ..
                }
            )
        });
        let project_roots = if revises_support {
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
                    results.push(successful_update_result(index, action, &entry));
                }
                Err(error) => results.push(json!({
                    "index": index,
                    "action": action,
                    "entryId": entry_id,
                    "updated": false,
                    "error": receipt_error(error),
                })),
            }
        }
        bounded_json_output(
            &call,
            json!({
                "updated": updated,
                "failed": results.len().saturating_sub(updated),
                "results": results,
            }),
        )
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
        let category = crate::memory_controls::change_category(store, &current).await;
        // Policy checks below must judge the revision this mutation replaces.
        if current.revision != mutation.expected_revision() {
            return Err(respond(BlackboardStoreError::RevisionConflict {
                expected: mutation.expected_revision(),
                actual: current.revision,
            }));
        }
        // A user rule is the user's own words: an agent may change its promotion or retire or
        // supersede it, but never rewrite it, and promotion keeps the user's provenance.
        let user_rule = current.value.kind == BlackboardKind::Instruction
            && current.value.provenance.kind == BlackboardProvenanceKind::User;
        let original = (
            current.value.kind,
            current.value.content.clone(),
            current.value.provenance.clone(),
        );
        let current_promotion = current.value.root_promotion;
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
            premises: current.value.premises,
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
                premises,
                ..
            } => {
                if verification == Some(BlackboardVerification::UserConfirmed) {
                    return Err(FunctionCallError::RespondToModel(
                        "userConfirmed is issued only from a host-observed user action and cannot be selected by the model"
                            .to_string(),
                    ));
                }
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
                    && premises.is_none()
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
                if user_rule && source_meaning_changed {
                    return Err(respond(
                        "a user rule keeps the user's exact words; record the new wording with userQuote and supersede this entry",
                    ));
                }
                if !user_rule && kind == Some(BlackboardKind::Instruction) {
                    return Err(respond(
                        "a rule must be the user's own words; record it as kind instruction with userQuote",
                    ));
                }
                let revised_verification = verification.unwrap_or(update.verification);
                if revised_verification == BlackboardVerification::UserConfirmed
                    && (source_meaning_changed || evidence.is_some() || premises.is_some())
                {
                    return Err(FunctionCallError::RespondToModel(
                        "changing user-confirmed meaning requires a new host-observed user action or an explicit verification downgrade"
                            .to_string(),
                    ));
                }
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
                if let Some(premises) = premises {
                    update.premises =
                        resolve_premises(&self.project_id, &self.services, project_roots, premises)
                            .await?;
                }
            }
            MutationArguments::Supersede {
                expected_revision,
                successor_entry_id,
                ..
            } => {
                update.expected_revision = expected_revision;
                update.state = BlackboardEntryState::Superseded;
                let successor_id = BlackboardEntryId::parse(successor_entry_id).map_err(respond)?;
                if user_rule {
                    let successor = store
                        .get_entry(&self.project_id, &successor_id)
                        .await
                        .map_err(respond)?;
                    if !successor.is_some_and(|successor| {
                        successor.value.kind == BlackboardKind::Instruction
                            && successor.value.provenance.kind == BlackboardProvenanceKind::User
                    }) {
                        return Err(respond(
                            "a user rule can be replaced only by the user's new rule in their own words",
                        ));
                    }
                }
                update.superseded_by = Some(successor_id);
            }
            MutationArguments::Retire {
                expected_revision, ..
            } => {
                update.expected_revision = expected_revision;
                update.state = BlackboardEntryState::Tombstoned;
            }
        }
        if user_rule
            && current_promotion != RootPromotion::Promoted
            && update.root_promotion == RootPromotion::Promoted
            && update.state == BlackboardEntryState::Active
        {
            return Err(respond(
                "a pending user rule applies only after the user states it as standing; it cannot be promoted",
            ));
        }
        let operation = match update.state {
            BlackboardEntryState::Tombstoned => Some(ChangeOperation::Forgotten),
            BlackboardEntryState::Superseded => Some(ChangeOperation::Invalidated),
            BlackboardEntryState::Active
                if update.kind != original.0 || update.content != original.1 =>
            {
                Some(ChangeOperation::Corrected)
            }
            BlackboardEntryState::Active
                if update.root_promotion == RootPromotion::Promoted
                    && current_promotion != RootPromotion::Promoted =>
            {
                Some(ChangeOperation::Promoted)
            }
            BlackboardEntryState::Active => None,
        };
        let change = operation.map(|operation| ChangeRecord {
            operation,
            origin: ChangeOrigin::ModelTool,
            category,
            action_id: None,
            thread_id: Some(self.thread_id.clone()),
            turn_id: None,
            group_id: None,
            preview: update.content.clone(),
        });
        // Promotion, supersession and retirement do not change who wrote the text; only a
        // revision of the text itself is the agent's.
        if update.kind == original.0 && update.content == original.1 {
            update.provenance = original.2;
        }
        let entry = store
            .update_entry_recorded(&self.project_id, &id, update, change.as_ref())
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

    fn exposure(&self) -> ToolExposure {
        // Specialized: discoverable through tool search instead of riding in every request.
        ToolExposure::DeferredModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: UPDATE_TOOL_NAME.to_string(),
            description: format!(
                "Apply 1-{MAX_MUTATIONS} revision-guarded lifecycle decisions to existing entries, each independent. setRootPromotion: promote a project-wide candidate (not verification). revise: change content, confidence, verification, importance, evidence, or premises (empty array clears); changing source-verified meaning needs fresh evidence_read receipts. supersede: a newer entry replaces an older one, including a \"current\" value that is no longer current. retire: obsolete with no successor. Nothing forgets or deletes: history, run results and conversation keep the text; if the user asks to forget, say so and state what remains. Copy a returned historicalFinding into materialHistoricalFindings when material to completion."
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

fn successful_update_result(
    index: usize,
    action: &str,
    entry: &BlackboardEntry,
) -> serde_json::Value {
    let mut result = json!({
        "index": index,
        "action": action,
        "entryId": entry.id.to_string(),
        "revision": entry.revision,
        "state": entry.state,
        "rootPromotion": entry.value.root_promotion,
        "updated": true,
    });
    if matches!(
        entry.state,
        BlackboardEntryState::Superseded | BlackboardEntryState::Tombstoned
    ) {
        result["historicalFinding"] = json!({
            "entryId": entry.id.to_string(),
            "revision": entry.revision,
        });
        result["retention"] = json!(RETENTION_DISCLOSURE);
    }
    result
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
        "enum": ["unverified", "sourceVerified", "disputed", "stale"],
        "description": "sourceVerified only with evidence receipts; userConfirmed is host-issued and unavailable here."
    });
    revise_properties["importance"] =
        json!({"type": "string", "enum": ["critical", "high", "normal", "low"]});
    revise_properties["rootPromotion"] =
        json!({"type": "string", "enum": ["notPromoted", "candidate", "promoted"]});
    revise_properties["evidence"] = evidence_schema();
    revise_properties["premises"] = premise_schema();
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
