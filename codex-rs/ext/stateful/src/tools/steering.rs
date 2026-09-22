use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::SteeringApplication;
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
use super::thread_run;

const QUERY_TOOL_NAME: &str = "steering_query";
const RECONCILE_TOOL_NAME: &str = "steering_reconcile";
const MAX_RETURNED_INPUT_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryArguments {
    cursor: Option<String>,
    limit: Option<u32>,
}

pub(super) struct SteeringQueryTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
}

impl SteeringQueryTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: QueryArguments = parse_arguments(&call)?;
        let run = thread_run(&self.project_id, &self.thread_id, &self.services).await?;
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
            description: "Read exact user steering and its acknowledgement/application state for the selected thread's active Stateful run. Current unresolved steering is already supplied in <stateful_run>; use this query only when that section says its view was omitted or shortened, or when historical reconciliation detail is needed.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "cursor": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100}
                },
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
    thread_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl SteeringReconcileTool {
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
        let selected_run = thread_run(&self.project_id, &self.thread_id, &self.services).await?;
        if current.value.run_id != selected_run.id {
            return Err(FunctionCallError::RespondToModel(
                "steering instruction does not belong to the selected thread's active run"
                    .to_string(),
            ));
        }
        let (status, resulting_strategy_revision, reason, run_revision): (_, _, _, Option<u64>) =
            match arguments.action {
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
                    let expected_run_revision =
                        arguments.expected_run_revision.ok_or_else(|| {
                            FunctionCallError::RespondToModel(
                                "apply requires expectedRunRevision".to_string(),
                            )
                        })?;
                    let strategy = arguments.strategy.ok_or_else(|| {
                        FunctionCallError::RespondToModel("apply requires strategy".to_string())
                    })?;
                    if selected_run.status == StatefulRunStatus::Pending {
                        return Err(FunctionCallError::RespondToModel(
                        "a pending Socratic run must be resumed by the user before steering can be applied to execution"
                            .to_string(),
                    ));
                    }
                    if selected_run.status.is_terminal() {
                        return Err(FunctionCallError::RespondToModel(
                            "cannot apply steering to a terminal run".to_string(),
                        ));
                    }
                    let (updated, applied) = store
                        .apply_steering(
                            &steering_id,
                            SteeringApplication {
                                expected_steering_revision: arguments.expected_revision,
                                expected_run_revision,
                                strategy,
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
                        event_sink.emit(StatefulEvent::SteeringUpdated {
                            project_id: applied.value.project_id.clone(),
                            run_id: applied.value.run_id.to_string(),
                            steering_id: applied.id.to_string(),
                            revision: applied.revision,
                        });
                    }
                    return Ok(Box::new(JsonToolOutput::new(json!({
                        "steeringId": applied.id.to_string(),
                        "status": steering_status_name(applied.status),
                        "revision": applied.revision,
                        "resultingStrategyRevision": applied.resulting_strategy_revision,
                        "runRevision": updated.revision,
                    }))));
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
            description: "Acknowledge exact user steering, atomically acknowledge and apply submitted steering through a guarded strategy revision, or reject it with a reason. Apply directly when the instruction can change strategy now; use acknowledge only when application must wait. Never silently drop steering.".to_string(),
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
