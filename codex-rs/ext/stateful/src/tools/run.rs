use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunUpdate;
use serde::Deserialize;
use serde_json::json;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::respond;
use super::thread_run;

const TOOL_NAME: &str = "stateful_run_update";
const MAX_FINAL_CHECKLIST_ITEMS: usize = 16;
const MAX_FINAL_CHECKLIST_ITEM_BYTES: usize = 640;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    expected_revision: u64,
    status: StatefulRunStatus,
    strategy: Option<String>,
    result: Option<String>,
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
        let arguments: Arguments = parse_arguments(&call)?;
        if !matches!(
            arguments.status,
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
        let final_obligation = if arguments.status == StatefulRunStatus::Completed {
            Some(
                runtime
                    .latest_obligation(&current.id)
                    .await
                    .map_err(respond)?
                    .ok_or_else(|| {
                        FunctionCallError::RespondToModel(
                            "completed requires a final semantic obligation that captures every material conclusion, implication, uncertainty, and blocker needed in the final answer"
                                .to_string(),
                        )
                    })?,
            )
        } else {
            None
        };
        let run = runtime
            .update_run(
                &current.id,
                StatefulRunUpdate {
                    expected_revision: arguments.expected_revision,
                    status: arguments.status,
                    strategy: arguments.strategy.or(current.strategy),
                    result: arguments.result.or(current.result),
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
        let (final_answer_checklist, omitted_checklist_items) = final_obligation
            .as_ref()
            .map(|obligation| final_answer_checklist(&obligation.value.packet))
            .unwrap_or_default();
        Ok(Box::new(JsonToolOutput::new(json!({
            "runId": run.id.to_string(),
            "status": status_name(run.status),
            "revision": run.revision,
            "strategyRevision": run.strategy_revision,
            "finalAnswerChecklist": final_answer_checklist,
            "omittedChecklistItems": omitted_checklist_items,
            "finalAnswerInstruction": (run.status == StatefulRunStatus::Completed).then_some(
                "Before replying, reconcile the persisted result and final prose against every checklist item. Include each material conclusion relevant to the user's request, preserve caveats and blockers, and cite the verified evidence. If omittedChecklistItems is nonzero, also use the full final obligation call you just made."
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
            description: "Persist a meaningful strategy/status change or final evidence-grounded result for the selected thread's active Stateful run. Completed requires a final semantic obligation and is terminal: finish every blackboard, relationship, steering, verification, and obligation operation first; ensure the result covers every material conclusion, implication, uncertainty, and blocker in that obligation; then make completed the final Stateful mutation. This cannot bypass a pending Socratic run or perform user-owned pause/cancel controls.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "expectedRevision": {"type": "integer", "minimum": 1},
                    "status": {"type": "string", "enum": ["running", "blocked", "completed", "failed"], "description": "Use completed only after a final semantic obligation captures all material answer content and all durable writes are finished; completion removes the active-run binding."},
                    "strategy": {"type": "string"},
                    "result": {"type": "string", "description": "For completed, the final evidence-grounded account after all durable writes and verification."}
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

fn final_answer_checklist(packet: &ObligationPacket) -> (Vec<serde_json::Value>, usize) {
    let candidates = [
        ("learning", &packet.learning),
        ("implication", &packet.implication),
        ("uncertainty", &packet.uncertainty),
        ("blocker", &packet.blockers),
    ];
    let total = candidates
        .iter()
        .map(|(_, items)| items.len())
        .sum::<usize>();
    let items = candidates
        .into_iter()
        .flat_map(|(category, items)| items.iter().map(move |item| (category, bounded_item(item))))
        .take(MAX_FINAL_CHECKLIST_ITEMS)
        .map(|(category, text)| json!({"category": category, "text": text}))
        .collect::<Vec<_>>();
    let omitted = total.saturating_sub(items.len());
    (items, omitted)
}

fn bounded_item(item: &str) -> String {
    if item.len() <= MAX_FINAL_CHECKLIST_ITEM_BYTES {
        return item.to_string();
    }
    let marker = "…";
    let mut boundary = MAX_FINAL_CHECKLIST_ITEM_BYTES.saturating_sub(marker.len());
    while !item.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{marker}", &item[..boundary])
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
