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
use super::scoped_run;
use super::stable_id;

const TOOL_NAME: &str = "obligation_update";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    run_id: String,
    idempotency_key: String,
    packet: ObligationPacket,
}

pub(super) struct ObligationUpdateTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl ObligationUpdateTool {
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
        let run = scoped_run(&self.project_id, arguments.run_id, &self.services).await?;
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
            description: "Record a compact semantic update when learning, strategy, uncertainty, blockers, or next work meaningfully changes. Explain significance; do not narrate routine tool activity.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "runId": {"type": "string"},
                    "idempotencyKey": {"type": "string"},
                    "packet": {
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
                    }
                },
                "required": ["runId", "idempotencyKey", "packet"],
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

fn string_list() -> serde_json::Value {
    json!({"type": "array", "items": {"type": "string"}, "maxItems": 32})
}
use std::sync::Arc;
