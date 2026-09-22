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
use serde::Deserialize;
use serde_json::json;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::respond;
use super::stable_id;
use super::thread_run;

const TOOL_NAME: &str = "obligation_update";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    idempotency_key: String,
    packet: ObligationPacket,
}

pub(super) struct ObligationUpdateTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl ObligationUpdateTool {
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
        let run = thread_run(&self.project_id, &self.thread_id, &self.services).await?;
        if run.status.is_terminal() {
            return Err(FunctionCallError::RespondToModel(
                "cannot update obligations for a terminal run".to_string(),
            ));
        }
        let obligation = self
            .services
            .runtime()
            .await
            .map_err(respond)?
            .append_obligation(
                stable_id("obligation", &self.project_id, &arguments.idempotency_key),
                NewObligation {
                    project_id: self.project_id.clone(),
                    run_id: run.id,
                    packet: arguments.packet,
                    provenance_source_id: call.call_id,
                },
            )
            .await
            .map_err(respond)?;
        if let Some(event_sink) = &self.event_sink {
            event_sink.emit(StatefulEvent::ObligationUpdated {
                project_id: obligation.value.project_id.clone(),
                run_id: obligation.value.run_id.to_string(),
                obligation_id: obligation.id.clone(),
                revision: obligation.revision,
            });
        }
        Ok(Box::new(JsonToolOutput::new(json!({
            "obligationId": obligation.id,
            "sequence": obligation.sequence,
            "revision": obligation.revision,
            "recorded": true,
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for ObligationUpdateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Record an intermediate compact semantic update for the selected thread's active Stateful run when learning, strategy, uncertainty, blockers, or next work meaningfully changes. Explain significance; do not narrate routine tool activity. When the work is ready to complete, put the final packet directly in stateful_run_update instead of spending a separate model turn here.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "idempotencyKey": {"type": "string"},
                    "packet": obligation_packet_schema()
                },
                "required": ["idempotencyKey", "packet"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static obligation schema: {error}")),
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

pub(super) fn obligation_packet_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "examined": string_list(),
            "rationale": string_list(),
            "learning": string_list(),
            "implication": string_list(),
            "strategy": string_list(),
            "changed": string_list(),
            "next": string_list(),
            "uncertainty": string_list(),
            "blockers": string_list(),
            "requestedJudgment": string_list()
        },
        "additionalProperties": false
    })
}

fn string_list() -> serde_json::Value {
    json!({"type": "array", "items": {"type": "string"}, "maxItems": 32})
}
use std::sync::Arc;
