use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::CompletionFence;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::completion::CompletionRequest;
use crate::completion::MAX_MATERIAL_ROOT_FINDINGS;
use crate::completion::prepare_completion;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;
use run_acceptance::AcceptanceDecision;

use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::provenance_source_id;
use super::respond;
use super::run_read::obligation_cursor;
use super::run_read::submitted_result_cursor;
use super::stable_id;
use super::thread_run;

/// Minimal valid call shown with every argument decoding rejection.
const EXAMPLE: &str = r#"{"expectedRevision":1,"status":"completed","completionDisposition":"noReusableLearning","result":"<final answer>","openIssues":[]}"#;

const COMPLETION_FENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const PAGED_RESULT_INSTRUCTION: &str = "The completed result is too large to return here. Read it exactly with stateful_run_read using section \"submittedResult\" and cursor submittedResultCursor, following nextCursor until it is null, then return it as the final answer without dropping, weakening, or changing any conclusion, caveat, uncertainty, or blocker. Copy opaque evidence identifiers only from finalAnswerChecklist; if omittedChecklistItems is nonzero, read the complete final obligation with stateful_run_read using section \"obligation\" and cursor finalObligationCursor, following nextCursor until it is null.";
const TOOL_NAME: &str = "stateful_run_update";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    expected_revision: u64,
    status: StatefulRunStatus,
    strategy: Option<String>,
    result: Option<String>,
    root_revision: Option<u64>,
    material_root_findings: Option<Vec<String>>,
    // Accept only an empty legacy field; historical completion publication is unsupported.
    material_historical_findings: Option<Vec<Value>>,
    completion_idempotency_key: Option<String>,
    final_obligation: Option<ObligationPacket>,
    /// Required with `completed`: every unresolved doubt, known discrepancy, failing check or
    /// open blocker the agent knows of; any entry ends the run blocked, not completed.
    open_issues: Option<Vec<String>>,
    #[serde(default)]
    completion_disposition: CompletionDisposition,
}

/// Whether a completed run carries reusable project learning. Lookups and answers
/// built entirely from existing state complete without the durable-learning ritual.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CompletionDisposition {
    #[default]
    DurableLearning,
    NoReusableLearning,
}

pub(super) struct StatefulRunUpdateTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
    visible_root: VisibleRootRegistry,
}

impl StatefulRunUpdateTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
        visible_root: VisibleRootRegistry,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
            projects,
            event_sink,
            visible_root,
        }
    }

    async fn acquire_fence(&self) -> Result<CompletionFence, FunctionCallError> {
        self.services
            .blackboard()
            .await
            .map_err(respond)?
            .acquire_completion_fence(COMPLETION_FENCE_TIMEOUT)
            .await
            .map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "completion could not lock project knowledge ({error}); nothing changed, retry the same stateful_run_update"
                ))
            })
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
        arguments: Arguments,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let Arguments {
            expected_revision,
            status,
            strategy,
            result,
            root_revision,
            material_root_findings,
            material_historical_findings,
            completion_idempotency_key,
            final_obligation,
            open_issues,
            completion_disposition,
        } = arguments;
        if material_historical_findings
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        {
            return Err(FunctionCallError::RespondToModel(
                "materialHistoricalFindings is unsupported; completion may select only current root findings".to_string(),
            ));
        }
        if !matches!(
            status,
            StatefulRunStatus::Running
                | StatefulRunStatus::Blocked
                | StatefulRunStatus::Completed
                | StatefulRunStatus::Failed
        ) {
            return Err(FunctionCallError::RespondToModel(
                "the model may only update a run to running, blocked, completed, or failed; pause, resume from Socratic pending, and cancel are user controls"
                    .to_string(),
            ));
        }
        let current = thread_run(&self.project_id, &self.thread_id, &self.services).await?;
        if current.status == StatefulRunStatus::Pending {
            return Err(FunctionCallError::RespondToModel(
                "a Socratic run must be resumed explicitly by the user before execution"
                    .to_string(),
            ));
        }
        let runtime = self.services.runtime().await.map_err(respond)?;
        let mut validated_obligation_sequence = None;
        if status == StatefulRunStatus::Completed {
            let (open_issues, sequence) = run_acceptance::declared_open_issues(
                open_issues.as_deref(),
                final_obligation.as_ref(),
                runtime,
                &current,
            )
            .await?;
            validated_obligation_sequence = sequence;
            if !open_issues.is_empty() {
                let run = self
                    .block_with_open_issues(
                        &current,
                        expected_revision,
                        result.as_deref(),
                        &open_issues,
                    )
                    .await?;
                return self.blocked_output(
                    &call,
                    &run,
                    "The completion declared unresolved issues or open blockers, so the host recorded this run as blocked with a partial result instead of completed. Report the partial result and each open issue to the user; resume work (status running) only to resolve them.",
                );
            }
        } else if open_issues.is_some() {
            return Err(FunctionCallError::RespondToModel(
                "openIssues is only valid when status is completed".to_string(),
            ));
        }
        let submitted_result = (status == StatefulRunStatus::Completed)
            .then(|| result.clone())
            .flatten();
        let no_reusable_learning = status == StatefulRunStatus::Completed
            && completion_disposition == CompletionDisposition::NoReusableLearning;
        if completion_disposition == CompletionDisposition::NoReusableLearning
            && status != StatefulRunStatus::Completed
        {
            return Err(FunctionCallError::RespondToModel(
                "completionDisposition noReusableLearning is only valid when status is completed"
                    .to_string(),
            ));
        }
        // Terminal completion holds the project database's writer lock from validation
        // through the runtime commit, so no project mutation can land in between.
        let mut fence = None;
        let (completion, final_obligation) = if no_reusable_learning {
            let durable_fields = [
                ("rootRevision", root_revision.is_some()),
                ("materialRootFindings", material_root_findings.is_some()),
                (
                    "completionIdempotencyKey",
                    completion_idempotency_key.is_some(),
                ),
                ("finalObligation", final_obligation.is_some()),
            ]
            .into_iter()
            .filter_map(|(field, present)| present.then_some(field))
            .collect::<Vec<_>>();
            if !durable_fields.is_empty() {
                return Err(FunctionCallError::RespondToModel(format!(
                    "completionDisposition noReusableLearning takes only expectedRevision, status, completionDisposition, result, and openIssues; remove {}, or use durableLearning when the run produced reusable project knowledge",
                    durable_fields.join(", ")
                )));
            }
            let knowledge_changed = fence
                .insert(self.acquire_fence().await?)
                .agent_knowledge_changed_since(&self.project_id, current.created_at_ms)
                .await
                .map_err(respond)?;
            if knowledge_changed {
                return Err(FunctionCallError::RespondToModel(
                    "agent-written project knowledge changed in this project during this run, so completion must use durableLearning and select the material finding (promote it first with blackboard_update_batch setRootPromotion if it is not in the root)".to_string(),
                ));
            }
            if result
                .as_deref()
                .is_none_or(|result| result.trim().is_empty())
            {
                return Err(FunctionCallError::RespondToModel(
                    "completed requires a final evidence-grounded result".to_string(),
                ));
            }
            (None, None)
        } else if status == StatefulRunStatus::Completed {
            let source_id = provenance_source_id(&call)?;
            if current.revision != expected_revision {
                return Err(FunctionCallError::RespondToModel(format!(
                    "run revision conflict: expected {expected_revision}, found {}",
                    current.revision
                )));
            }
            let material_root_findings = material_root_findings.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    format!("completed requires materialRootFindings; pass at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases directly material to the outcome. If finalObligation.learning is non-empty, select at least one current root alias that preserves the reusable learning. Pass [] only when the run produced no reusable project learning")
                )
            })?;
            let root_revision = root_revision.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires rootRevision from the current project intelligence World State"
                        .to_string(),
                )
            })?;
            let result = result.as_deref().ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires a final evidence-grounded result".to_string(),
                )
            })?;
            let final_obligation = final_obligation.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires finalObligation with every material conclusion, implication, uncertainty, and blocker needed in the persisted result and final answer"
                        .to_string(),
                )
            })?;
            let completion_idempotency_key = completion_idempotency_key.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires completionIdempotencyKey for the final obligation and terminal update"
                        .to_string(),
                )
            })?;
            let project = self
                .projects
                .read_project(self.project_id.clone())
                .await
                .map_err(respond)?
                .ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "selected project no longer exists".to_string(),
                    )
                })?;
            let project_roots = project
                .roots
                .iter()
                .map(|root| std::path::PathBuf::from(&root.path))
                .collect::<Vec<_>>();
            fence = Some(self.acquire_fence().await?);
            let visible_root = self.visible_root.get(&self.thread_id);
            let completion = prepare_completion(
                &self.services,
                CompletionRequest {
                    project_id: &self.project_id,
                    thread_id: &self.thread_id,
                    project_roots: &project_roots,
                    result,
                    packet: &final_obligation,
                    root_revision,
                    material_root_findings: &material_root_findings,
                    visible_root: visible_root.as_ref(),
                },
            )
            .await?;
            let obligation = (
                stable_id("obligation", &self.project_id, &completion_idempotency_key),
                NewObligation {
                    project_id: self.project_id.clone(),
                    run_id: current.id.clone(),
                    packet: final_obligation,
                    provenance_source_id: source_id,
                },
            );
            (Some(completion), Some(obligation))
        } else {
            if material_root_findings.is_some()
                || root_revision.is_some()
                || completion_idempotency_key.is_some()
                || final_obligation.is_some()
            {
                return Err(FunctionCallError::RespondToModel(
                    "rootRevision, materialRootFindings, completionIdempotencyKey, and finalObligation are only valid when status is completed".to_string(),
                ));
            }
            (None, None)
        };
        // The acceptance gate runs under the completion fence, after the disposition's own
        // validation, so its leased verification attempt and the terminal commit see one
        // consistent project state.
        let acceptance = if status == StatefulRunStatus::Completed {
            if current.revision != expected_revision {
                return Err(FunctionCallError::RespondToModel(format!(
                    "run revision conflict: expected {expected_revision}, found {}",
                    current.revision
                )));
            }
            match self
                .acceptance_decision(
                    &current,
                    result.as_deref(),
                    local_executor(&call),
                    validated_obligation_sequence,
                )
                .await?
            {
                AcceptanceDecision::Proceed { commit, basis } => Some((commit, basis)),
                AcceptanceDecision::Refused(message) => {
                    return Err(FunctionCallError::RespondToModel(message));
                }
                AcceptanceDecision::Blocked(run) => {
                    return self.blocked_output(
                        &call,
                        &run,
                        "Completion was refused repeatedly without acceptance progress, so the host moved this run to blocked as a partial result. Do not claim the work is complete: tell the user which acceptance gates remain unmet (read them with stateful_run_read section \"acceptance\") and what was delivered.",
                    );
                }
            }
        } else {
            None
        };
        let acceptance_basis = acceptance
            .as_ref()
            .map(|(_, basis)| basis.clone())
            .unwrap_or_default();
        let durable_result = completion
            .as_ref()
            .map(|completion| completion.result.clone())
            .or(result);
        let update = StatefulRunUpdate {
            expected_revision,
            status,
            strategy: strategy.or(current.strategy),
            result: match &acceptance {
                Some((_, basis)) => durable_result
                    .map(|durable| run_acceptance::with_acceptance_basis(durable, basis)),
                None => durable_result,
            }
            .or(current.result),
        };
        let (run, final_obligation) = if let Some((commit, _)) = &acceptance {
            runtime
                .complete_run_with_acceptance(&current.id, update, commit, final_obligation)
                .await
                .map_err(respond)?
        } else {
            (
                runtime
                    .update_run(&current.id, update)
                    .await
                    .map_err(respond)?,
                None,
            )
        };
        if let Some(fence) = fence
            && let Err(error) = fence.release().await
        {
            // The run is already terminal; an unreleased fence is closed on drop.
            tracing::warn!("failed to release the Stateful completion fence: {error}");
        }
        if let Some(event_sink) = &self.event_sink {
            if let Some(obligation) = &final_obligation {
                event_sink.emit(StatefulEvent::ObligationUpdated {
                    project_id: obligation.value.project_id.clone(),
                    run_id: obligation.value.run_id.to_string(),
                    obligation_id: obligation.id.clone(),
                    revision: obligation.revision,
                });
            }
            event_sink.emit(StatefulEvent::RunUpdated {
                project_id: run.value.project_id.clone(),
                run_id: run.id.to_string(),
                revision: run.revision,
            });
        }
        let final_answer_checklist = completion
            .as_ref()
            .map(|completion| completion.checklist.clone())
            .unwrap_or_default();
        let omitted_checklist_items = completion
            .as_ref()
            .map_or(0, |completion| completion.omitted_checklist_items);
        let mut output = json!({
            "runId": run.id.to_string(),
            "status": status_name(run.status),
            "revision": run.revision,
            "strategyRevision": run.strategy_revision,
            "finalAnswerChecklist": final_answer_checklist,
            "omittedChecklistItems": omitted_checklist_items,
            "submittedResult": submitted_result,
            "finalAnswerInstruction": (run.status == StatefulRunStatus::Completed).then_some(
                "Return submittedResult as the final answer without dropping, weakening, or changing any conclusion, caveat, uncertainty, or blocker. You may improve formatting and exact-source links. Copy opaque evidence identifiers only from finalAnswerChecklist; never reconstruct or abbreviate them from memory. For a Windows drive path, use the exact C:/... Markdown target form and never rewrite it as /C:/.... Use finalAnswerChecklist to confirm that the visible answer preserves the durable completion basis; if omittedChecklistItems is nonzero, read the complete final obligation with stateful_run_read using section \"obligation\" and cursor finalObligationCursor, following nextCursor until it is null."
            ),
        });
        if !acceptance_basis.is_empty() {
            output["acceptanceBasis"] = json!(acceptance_basis);
            output["omittedAcceptanceBasis"] = json!(0);
            output["acceptanceInstruction"] = json!(
                "The acceptance basis is part of the durable result. Disclose every UNVERIFIED, manual-observation and dismissed item in the final answer; never present them as verified."
            );
        }
        // The run is already durable here, so the response is packed to fit rather than
        // refused: an oversized result moves behind an exact paged read, then checklist
        // items drop from the end with an honest omitted count.
        // Pack to a fixed point: whenever checklist items are omitted (by completion or
        // by this loop), the final obligation must stay readable through its cursor, and
        // adding that cursor may itself force another item out.
        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let mut result_paged = false;
        loop {
            if output["omittedChecklistItems"].as_u64().unwrap_or_default() > 0
                && output["finalObligationCursor"].is_null()
                && let Some(obligation) = &final_obligation
            {
                output["finalObligationCursor"] = json!(obligation_cursor(&run, obligation)?);
            }
            if output.to_string().len() <= budget {
                break;
            }
            if !result_paged
                && let Some(cursor) = submitted_result
                    .as_deref()
                    .and_then(|submitted| submitted_result_cursor(&run, submitted))
            {
                result_paged = true;
                output["submittedResult"] = Value::Null;
                output["submittedResultCursor"] = json!(cursor);
                output["finalAnswerInstruction"] = json!(PAGED_RESULT_INSTRUCTION);
                continue;
            }
            if output["finalAnswerChecklist"]
                .as_array_mut()
                .and_then(Vec::pop)
                .is_some()
            {
                let omitted = output["omittedChecklistItems"].as_u64().unwrap_or_default();
                output["omittedChecklistItems"] = json!(omitted + 1);
                continue;
            }
            // The full basis is in the durable result and the paged acceptance read.
            if output
                .get_mut("acceptanceBasis")
                .and_then(Value::as_array_mut)
                .and_then(Vec::pop)
                .is_none()
            {
                break;
            }
            let omitted = output["omittedAcceptanceBasis"]
                .as_u64()
                .unwrap_or_default();
            output["omittedAcceptanceBasis"] = json!(omitted + 1);
        }
        bounded_json_output(&call, output)
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for StatefulRunUpdateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn exposure(&self) -> ToolExposure {
        // Prose-bearing mutations stay out of nested code mode: model-written JS
        // string literals break on quotes inside long semantic fields.
        ToolExposure::DirectModelOnly
    }

    fn spec(&self) -> ToolSpec {
        let mut final_obligation = super::obligation::obligation_packet_schema();
        final_obligation["description"] = json!(
            "durableLearning only: the material conclusions, implications, uncertainties and blockers."
        );
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: format!("Change the active Stateful run's strategy or status, or complete it. Every completion passes openIssues. noReusableLearning completion (answer from existing knowledge, a narrow citation, or cheap to recompute): exactly expectedRevision, status completed, completionDisposition noReusableLearning, result. durableLearning completion (default), after every other durable write: expectedRevision, status completed, completionIdempotencyKey, finalObligation, result, rootRevision, materialRootFindings (at most {MAX_MATERIAL_ROOT_FINDINGS} E aliases), selecting a finding that preserves any finalObligation.learning. Rejected while steering is unresolved or a revision changed. Cannot begin a pending Socratic run, pause, or cancel."),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "expectedRevision": {"type": "integer", "minimum": 1, "description": "The run revision from <stateful_run>, not rootRevision."},
                    "status": {"type": "string", "enum": ["running", "blocked", "completed", "failed"], "description": "completed only after all other durable writes."},
                    "strategy": {"type": "string"},
                    "result": {"type": "string", "description": "For completed, the concise final evidence-grounded narrative."},
                    "rootRevision": {"type": "integer", "minimum": 0, "description": "durableLearning only; the root blackboard's project intelligence revision."},
                    "materialRootFindings": {"type": "array", "items": {"type": "string", "pattern": "^E[1-9][0-9]*$"}, "maxItems": MAX_MATERIAL_ROOT_FINDINGS, "description": "durableLearning only; [] only when nothing reusable was learned."},
                    "completionIdempotencyKey": {"type": "string", "description": "durableLearning only; reuse only for an identical retry."},
                    "finalObligation": final_obligation,
                    "completionDisposition": {"type": "string", "enum": ["durableLearning", "noReusableLearning"], "description": "Defaults to durableLearning."},
                    "openIssues": {"type": "array", "items": {"type": "string"}, "maxItems": 16, "description": "completed: open doubts, discrepancies, failing checks, blockers ([] if none); any entry blocks the run."}
                },
                "required": ["expectedRevision", "status"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static run update schema: {error}")),
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
        Box::pin(self.handle_guided(call))
    }
}

/// Whether every environment of the call is the app-server host itself, the only executor
/// whose files the host can read as artifact evidence.
pub(super) fn local_executor(call: &ToolCall<'_>) -> bool {
    const LOCAL_ENVIRONMENT_ID: &str = "local";
    !call.environments.is_empty()
        && call
            .environments
            .iter()
            .all(|environment| environment.environment_id == LOCAL_ENVIRONMENT_ID)
}

fn status_name(status: StatefulRunStatus) -> &'static str {
    match status {
        StatefulRunStatus::Pending => "pending",
        StatefulRunStatus::Running => "running",
        StatefulRunStatus::Paused => "paused",
        StatefulRunStatus::Completed => "completed",
        StatefulRunStatus::Cancelled => "cancelled",
        StatefulRunStatus::Blocked => "blocked",
        StatefulRunStatus::Failed => "failed",
        StatefulRunStatus::Answered => "answered",
    }
}
use std::sync::Arc;

#[path = "run_guard.rs"]
mod guard;

#[path = "run_acceptance.rs"]
mod run_acceptance;

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
