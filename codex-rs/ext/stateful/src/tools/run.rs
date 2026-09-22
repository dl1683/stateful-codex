use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
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
        let completion = if status == StatefulRunStatus::Completed {
            let material_root_findings = material_root_findings.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires materialRootFindings; pass every materially relevant E alias from the current root blackboard, or [] only after determining none is material"
                        .to_string(),
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
            Some(
                prepare_completion(
                    &self.project_id,
                    &self.services,
                    result,
                    &runtime
                        .latest_obligation(&current.id)
                        .await
                        .map_err(respond)?
                        .ok_or_else(|| {
                        FunctionCallError::RespondToModel(
                            "completed requires a final semantic obligation that captures every material conclusion, implication, uncertainty, and blocker needed in the final answer"
                                .to_string(),
                        )
                    })?
                    .value
                    .packet,
                    root_revision,
                    &material_root_findings,
                )
                .await?,
            )
        } else {
            if material_root_findings.is_some() || root_revision.is_some() {
                return Err(FunctionCallError::RespondToModel(
                    "rootRevision and materialRootFindings are only valid when status is completed"
                        .to_string(),
                ));
            }
            None
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
            description: "Persist a meaningful strategy/status change or final evidence-grounded result for the selected thread's active Stateful run. Completed requires a final semantic obligation plus rootRevision and an explicit materialRootFindings selection: finish every blackboard, relationship, steering, verification, and obligation operation first; copy the current project intelligence revision and select every materially relevant E alias from that root; then make completed the final Stateful mutation. The tool rejects a changed root before mutation and appends the selected findings and bounded final-obligation conclusions to the durable result. This cannot bypass a pending Socratic run or perform user-owned pause/cancel controls.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "expectedRevision": {"type": "integer", "minimum": 1},
                    "status": {"type": "string", "enum": ["running", "blocked", "completed", "failed"], "description": "Use completed only after a final semantic obligation captures all material answer content and all durable writes are finished; completion removes the active-run binding."},
                    "strategy": {"type": "string"},
                    "result": {"type": "string", "description": "For completed, the concise final evidence-grounded narrative after all durable writes and verification. The tool appends the structured completion basis."},
                    "rootRevision": {"type": "integer", "minimum": 0, "description": "Required for completed. Copy the project intelligence revision shown with the current root blackboard; completion fails before mutation if it changed."},
                    "materialRootFindings": {"type": "array", "items": {"type": "string", "pattern": "^E[1-9][0-9]*$"}, "maxItems": MAX_MATERIAL_ROOT_FINDINGS, "description": "Required for completed. Include every materially relevant E alias shown in the root blackboard at rootRevision; use [] only after determining no root finding is material to the requested outcome."}
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
