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
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::respond;
use super::scoped_run;

const TOOL_NAME: &str = "stateful_run_update";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    run_id: String,
    expected_revision: u64,
    status: StatefulRunStatus,
    strategy: Option<String>,
    result: Option<String>,
}

pub(super) struct StatefulRunUpdateTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl StatefulRunUpdateTool {
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
        let current = scoped_run(&self.project_id, arguments.run_id, &self.services).await?;
        if current.status == StatefulRunStatus::Pending {
            return Err(FunctionCallError::RespondToModel(
                "a Socratic run must be resumed explicitly by the user before execution"
                    .to_string(),
            ));
        }
        let run = self
            .services
            .runtime()
            .await
            .map_err(respond)?
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
        Ok(Box::new(JsonToolOutput::new(json!({
            "runId": run.id.to_string(),
            "status": status_name(run.status),
            "revision": run.revision,
            "strategyRevision": run.strategy_revision,
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
            description: "Persist a meaningful strategy/status change or final evidence-grounded result. This cannot bypass a pending Socratic run or perform user-owned pause/cancel controls.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "runId": {"type": "string"},
                    "expectedRevision": {"type": "integer", "minimum": 1},
                    "status": {"type": "string", "enum": ["running", "blocked", "completed", "failed"]},
                    "strategy": {"type": "string"},
                    "result": {"type": "string"}
                },
                "required": ["runId", "expectedRevision", "status"],
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
