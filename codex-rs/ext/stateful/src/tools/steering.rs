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
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::SteeringId;
use codex_stateful_runtime::SteeringStatus;
use codex_stateful_runtime::SteeringUpdate;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::fits_response;
use super::parse_arguments;
use super::respond;
use super::scoped_run;

const QUERY_TOOL_NAME: &str = "steering_query";
const RECONCILE_TOOL_NAME: &str = "steering_reconcile";
const MAX_RETURNED_INPUT_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryArguments {
    run_id: String,
    cursor: Option<String>,
    limit: Option<u32>,
}

pub(super) struct SteeringQueryTool {
    project_id: String,
    services: ProjectIntelligenceServices,
}

impl SteeringQueryTool {
    pub(super) fn new(project_id: String, services: ProjectIntelligenceServices) -> Self {
        Self {
            project_id,
            services,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: QueryArguments = parse_arguments(&call)?;
        let run = scoped_run(&self.project_id, arguments.run_id, &self.services).await?;
        let cursor = arguments
            .cursor
            .map(SteeringId::parse)
            .transpose()
            .map_err(respond)?;
        let limit = arguments.limit.unwrap_or(20);
        if limit == 0 || limit > 100 {
            return Err(FunctionCallError::RespondToModel(
                "limit must be between 1 and 100".to_string(),
            ));
        }
        let data = self
            .services
            .runtime()
            .await
            .map_err(respond)?
            .list_steering(&run.id, cursor.as_ref(), limit.saturating_add(1))
            .await
            .map_err(respond)?;
        let mut rendered = Vec::new();
        for instruction in data.iter().take(limit as usize) {
            let item = steering_json(instruction);
            let mut candidate = rendered.clone();
            candidate.push(item.clone());
            if !fits_response(
                &json!({"data": candidate}),
                call.response_byte_budget(MAX_RESPONSE_BYTES),
            ) {
                break;
            }
            rendered.push(item);
        }
        let has_more = data.len() > rendered.len();
        let next_cursor = has_more
            .then(|| rendered.last())
            .flatten()
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str);
        Ok(Box::new(JsonToolOutput::new(json!({
            "data": rendered,
            "nextCursor": next_cursor,
            "truncated": has_more,
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for SteeringQueryTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(QUERY_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: QUERY_TOOL_NAME.to_string(),
            description: "Read exact user steering and its acknowledgement/application state. Check unresolved steering before choosing or revising strategy.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "runId": {"type": "string"},
                    "cursor": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
                "required": ["runId"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static steering query schema: {error}")),
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

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReconcileAction {
    Acknowledge,
    Apply,
    Reject,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconcileArguments {
    steering_id: String,
    expected_revision: u64,
    action: ReconcileAction,
    expected_run_revision: Option<u64>,
    strategy: Option<String>,
    reason: Option<String>,
}

pub(super) struct SteeringReconcileTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl SteeringReconcileTool {
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
        let arguments: ReconcileArguments = parse_arguments(&call)?;
        let steering_id = SteeringId::parse(arguments.steering_id).map_err(respond)?;
        let store = self.services.runtime().await.map_err(respond)?;
        let current = store
            .get_steering(&steering_id)
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "steering instruction not found: {steering_id}"
                ))
            })?;
        if current.value.project_id != self.project_id {
            return Err(FunctionCallError::RespondToModel(
                "steering instruction does not belong to the selected project".to_string(),
            ));
        }
        let (status, resulting_strategy_revision, reason, run_revision) = match arguments.action {
            ReconcileAction::Acknowledge => {
                if arguments.expected_run_revision.is_some()
                    || arguments.strategy.is_some()
                    || arguments.reason.is_some()
                {
                    return Err(FunctionCallError::RespondToModel(
                        "acknowledge accepts only steeringId, expectedRevision, and action"
                            .to_string(),
                    ));
                }
                (SteeringStatus::Acknowledged, None, None, None)
            }
            ReconcileAction::Reject => (SteeringStatus::Rejected, None, arguments.reason, None),
            ReconcileAction::Apply => {
                let expected_run_revision = arguments.expected_run_revision.ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "apply requires expectedRunRevision".to_string(),
                    )
                })?;
                let strategy = arguments.strategy.ok_or_else(|| {
                    FunctionCallError::RespondToModel("apply requires strategy".to_string())
                })?;
                let run = scoped_run(
                    &self.project_id,
                    current.value.run_id.to_string(),
                    &self.services,
                )
                .await?;
                if run.status == StatefulRunStatus::Pending {
                    return Err(FunctionCallError::RespondToModel(
                        "a pending Socratic run must be resumed by the user before steering can be applied to execution"
                            .to_string(),
                    ));
                }
                if run.status.is_terminal() {
                    return Err(FunctionCallError::RespondToModel(
                        "cannot apply steering to a terminal run".to_string(),
                    ));
                }
                let updated = store
                    .update_run(
                        &run.id,
                        StatefulRunUpdate {
                            expected_revision: expected_run_revision,
                            status: run.status,
                            strategy: Some(strategy),
                            result: run.result,
                        },
                    )
                    .await
                    .map_err(respond)?;
                if let Some(event_sink) = &self.event_sink {
                    event_sink.emit(StatefulEvent::RunUpdated {
                        project_id: updated.value.project_id.clone(),
                        run_id: updated.id.to_string(),
                        revision: updated.revision,
                    });
                }
                (
                    SteeringStatus::Applied,
                    Some(updated.strategy_revision),
                    None,
                    Some(updated.revision),
                )
            }
        };
        let updated = store
            .update_steering(
                &steering_id,
                SteeringUpdate {
                    expected_revision: arguments.expected_revision,
                    status,
                    resulting_strategy_revision,
                    reason,
                },
            )
            .await
            .map_err(respond)?;
        if let Some(event_sink) = &self.event_sink {
            event_sink.emit(StatefulEvent::SteeringUpdated {
                project_id: updated.value.project_id.clone(),
                run_id: updated.value.run_id.to_string(),
                steering_id: updated.id.to_string(),
                revision: updated.revision,
            });
        }
        Ok(Box::new(JsonToolOutput::new(json!({
            "steeringId": updated.id.to_string(),
            "status": steering_status_name(updated.status),
            "revision": updated.revision,
            "resultingStrategyRevision": updated.resulting_strategy_revision,
            "runRevision": run_revision,
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for SteeringReconcileTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(RECONCILE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: RECONCILE_TOOL_NAME.to_string(),
            description: "Acknowledge exact user steering, visibly apply it through a guarded strategy revision, or reject it with a reason. Never silently drop steering.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "steeringId": {"type": "string"},
                    "expectedRevision": {"type": "integer", "minimum": 1},
                    "action": {"type": "string", "enum": ["acknowledge", "apply", "reject"]},
                    "expectedRunRevision": {"type": "integer", "minimum": 1},
                    "strategy": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["steeringId", "expectedRevision", "action"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static steering reconciliation schema: {error}")),
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

fn steering_json(instruction: &StatefulSteering) -> Value {
    let (input, input_truncated) = bounded_text(&instruction.value.input, MAX_RETURNED_INPUT_BYTES);
    json!({
        "id": instruction.id.to_string(),
        "input": input,
        "inputTruncated": input_truncated,
        "affectedObligationIds": instruction.value.affected_obligation_ids,
        "status": steering_status_name(instruction.status),
        "resultingStrategyRevision": instruction.resulting_strategy_revision,
        "reason": instruction.reason,
        "revision": instruction.revision,
    })
}

fn bounded_text(value: &str, maximum: usize) -> (&str, bool) {
    if value.len() <= maximum {
        return (value, false);
    }
    let mut boundary = maximum;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (&value[..boundary], true)
}

fn steering_status_name(status: SteeringStatus) -> &'static str {
    match status {
        SteeringStatus::Submitted => "submitted",
        SteeringStatus::Acknowledged => "acknowledged",
        SteeringStatus::Applied => "applied",
        SteeringStatus::Rejected => "rejected",
    }
}
use std::sync::Arc;
