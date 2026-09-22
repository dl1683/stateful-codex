use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunUpdate;
use serde::Deserialize;
use serde_json::json;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::completion::MAX_MATERIAL_ROOT_FINDINGS;
use crate::completion::prepare_completion;
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::respond;
use super::stable_id;
use super::thread_run;

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
    completion_idempotency_key: Option<String>,
    final_obligation: Option<ObligationPacket>,
}

pub(super) struct StatefulRunUpdateTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl StatefulRunUpdateTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
            event_sink,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let Arguments {
            expected_revision,
            status,
            strategy,
            result,
            root_revision,
            material_root_findings,
            completion_idempotency_key,
            final_obligation,
        } = parse_arguments(&call)?;
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
        let (completion, final_obligation) = if status == StatefulRunStatus::Completed {
            if current.revision != expected_revision {
                return Err(FunctionCallError::RespondToModel(format!(
                    "run revision conflict: expected {expected_revision}, found {}",
                    current.revision
                )));
            }
            let material_root_findings = material_root_findings.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    format!("completed requires materialRootFindings; pass at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases directly material to the outcome, preserve additional conclusions in the final semantic obligation, or pass [] only after determining no root finding is material")
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
            let completion = prepare_completion(
                &self.project_id,
                &self.services,
                result,
                &final_obligation,
                root_revision,
                &material_root_findings,
            )
            .await?;
            let obligation = runtime
                .append_obligation(
                    stable_id("obligation", &self.project_id, &completion_idempotency_key),
                    NewObligation {
                        project_id: self.project_id.clone(),
                        run_id: current.id.clone(),
                        packet: final_obligation,
                        provenance_source_id: call.call_id.clone(),
                    },
                )
                .await
                .map_err(respond)?;
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
        let run = runtime
            .update_run(
                &current.id,
                StatefulRunUpdate {
                    expected_revision,
                    status,
                    strategy: strategy.or(current.strategy),
                    result: completion
                        .as_ref()
                        .map(|completion| completion.result.clone())
                        .or(result)
                        .or(current.result),
                },
            )
            .await
            .map_err(respond)?;
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
        Ok(Box::new(JsonToolOutput::new(json!({
            "runId": run.id.to_string(),
            "status": status_name(run.status),
            "revision": run.revision,
            "strategyRevision": run.strategy_revision,
            "finalAnswerChecklist": final_answer_checklist,
            "omittedChecklistItems": omitted_checklist_items,
            "finalAnswerInstruction": (run.status == StatefulRunStatus::Completed).then_some(
                "The durable result now contains this bounded completion basis. Before replying, reconcile the final prose against every checklist item, preserve caveats and blockers, and cite the verified evidence. If omittedChecklistItems is nonzero, also use the full final obligation call you just made."
            ),
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for StatefulRunUpdateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: format!("Persist a meaningful strategy/status change or final evidence-grounded result for the selected thread's active Stateful run. expectedRevision is the current run revision, while rootRevision is the separate project intelligence revision. Completed records finalObligation and the terminal result in one tool operation: finish every blackboard, relationship, steering, and verification operation first; select at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases directly material to the outcome; then supply completionIdempotencyKey, finalObligation, and the result in this single final Stateful mutation. The tool rejects changed run or root revisions before persistence and appends the selected findings and bounded final-obligation conclusions to the durable result. This cannot bypass a pending Socratic run or perform user-owned pause/cancel controls."),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "expectedRevision": {"type": "integer", "minimum": 1, "description": "Copy the Run revision from the Stateful run World State. This is not the project intelligence revision used by rootRevision."},
                    "status": {"type": "string", "enum": ["running", "blocked", "completed", "failed"], "description": "Use completed only after all other durable writes are finished; the same call must carry the final semantic obligation and removes the active-run binding."},
                    "strategy": {"type": "string"},
                    "result": {"type": "string", "description": "For completed, the concise final evidence-grounded narrative after all durable writes and verification. The tool appends the structured completion basis."},
                    "rootRevision": {"type": "integer", "minimum": 0, "description": "Required for completed. Copy the project intelligence revision shown with the current root blackboard; completion fails before mutation if it changed."},
                    "materialRootFindings": {"type": "array", "items": {"type": "string", "pattern": "^E[1-9][0-9]*$"}, "maxItems": MAX_MATERIAL_ROOT_FINDINGS, "description": format!("Required for completed. Select at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases shown at rootRevision that are directly material to the requested outcome; preserve additional material conclusions in finalObligation. Use [] only after determining no root finding is material.")},
                    "completionIdempotencyKey": {"type": "string", "description": "Required for completed. Reuse only when retrying this identical final obligation and terminal result."},
                    "finalObligation": super::obligation::obligation_packet_schema()
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
        Box::pin(self.handle_call(call))
    }
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
    }
}
use std::sync::Arc;
