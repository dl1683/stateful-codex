mod blackboard;
mod blackboard_evidence;
mod blackboard_premises;
mod blackboard_update;
mod blackboard_write;
mod context_map;
mod conversation_read;
mod evidence;
mod obligation;
mod run;
mod run_read;
mod steering;

use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_stateful_runtime::StatefulRun;
use codex_thread_store::ThreadStore;
use sha2::Digest;
use sha2::Sha256;

use crate::StatefulEventSink;
use crate::limits::MAX_MODEL_ITEM_BYTES;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

/// Serialized bound for every Stateful tool result: one UTF-8 byte is the worst case per
/// token, so 9,000 bytes stays under the 10K-token model item limit with framing room.
const MAX_RESPONSE_BYTES: usize = MAX_MODEL_ITEM_BYTES;
/// Serialized bound for one error string inside a result receipt.
const MAX_RECEIPT_ERROR_BYTES: usize = 240;

pub(super) fn project_intelligence_tools(
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
    visible_root: VisibleRootRegistry,
) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
    let mut tools: Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> = vec![
        Arc::new(blackboard::BlackboardQueryTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
            projects.clone(),
            visible_root.clone(),
        )),
        Arc::new(blackboard_write::BlackboardBatchRecordTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
            projects.clone(),
            event_sink.clone(),
        )),
        Arc::new(blackboard_update::BlackboardUpdateTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
            projects.clone(),
            event_sink.clone(),
        )),
        Arc::new(blackboard_write::BlackboardRelateTool::new(
            project_id.clone(),
            services.clone(),
            event_sink.clone(),
        )),
        Arc::new(conversation_read::ConversationReadTool::new(
            project_id.clone(),
            projects.clone(),
        )),
        Arc::new(context_map::ContextMapQueryTool::new(
            project_id.clone(),
            services.clone(),
            projects.clone(),
        )),
        Arc::new(context_map::ContextMapRefreshTool::new(
            project_id.clone(),
            services.clone(),
            projects.clone(),
        )),
        Arc::new(evidence::EvidenceReadTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
            projects.clone(),
        )),
    ];
    tools.extend([
        Arc::new(obligation::ObligationUpdateTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
            event_sink.clone(),
        )) as Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>,
        Arc::new(run::StatefulRunUpdateTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
            projects,
            event_sink.clone(),
            visible_root,
        )),
        Arc::new(run_read::StatefulRunReadTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
        )),
        Arc::new(steering::SteeringQueryTool::new(
            project_id.clone(),
            thread_id.clone(),
            services.clone(),
        )),
        Arc::new(steering::SteeringReconcileTool::new(
            project_id, thread_id, services, event_sink,
        )),
    ]);
    tools
}

fn parse_arguments<T: for<'de> serde::Deserialize<'de>>(
    call: &ToolCall<'_>,
) -> Result<T, codex_extension_api::FunctionCallError> {
    let arguments = call.function_arguments()?;
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    serde_json::from_str(arguments)
        .map_err(|error| codex_extension_api::FunctionCallError::RespondToModel(error.to_string()))
}

/// Emits a JSON result only when its serialized text fits the call's budget. Callers
/// that mutate state must size their output before mutating; this is the backstop.
fn bounded_json_output(
    call: &ToolCall<'_>,
    value: serde_json::Value,
) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
    let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
    if value.to_string().len() > budget {
        return Err(FunctionCallError::RespondToModel(format!(
            "tool result withheld: it exceeded the {budget}-byte model item bound. Any change this call made was applied; query the affected state instead of retrying."
        )));
    }
    Ok(Box::new(codex_extension_api::JsonToolOutput::new(value)))
}

/// Bounds an error message for a receipt so its serialized form stays within
/// `MAX_RECEIPT_ERROR_BYTES`.
fn receipt_error(error: impl std::fmt::Display) -> String {
    let mut message = error.to_string();
    while serde_json::Value::String(message.clone()).to_string().len() > MAX_RECEIPT_ERROR_BYTES {
        let mut end = message.len().saturating_sub(16);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push('…');
    }
    message
}

/// Refuses a batch before any mutation when its worst-case receipts could not all be
/// returned in one bounded response, so a caller never loses a receipt for a write.
fn preflight_receipts(
    call: &ToolCall<'_>,
    worst_case: &serde_json::Value,
) -> Result<(), FunctionCallError> {
    let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
    if worst_case.to_string().len() > budget {
        return Err(FunctionCallError::RespondToModel(format!(
            "nothing was written: this batch's receipts might not fit one {budget}-byte result. Split it into smaller batches."
        )));
    }
    Ok(())
}

/// Placeholders sized to the largest value a receipt field can hold.
fn worst_receipt_error() -> String {
    "x".repeat(MAX_RECEIPT_ERROR_BYTES - 2)
}

fn worst_identifier(supplied: &str) -> String {
    format!(
        "{supplied}{}",
        "x".repeat(128usize.saturating_sub(supplied.len()))
    )
}

fn fits_response(value: &serde_json::Value, byte_budget: usize) -> bool {
    serde_json::to_vec(value).is_ok_and(|serialized| serialized.len() <= byte_budget)
}

fn stable_id(prefix: &str, project_id: &str, idempotency_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(idempotency_key.as_bytes());
    let digest = hasher.finalize();
    format!("stateful-{prefix}-{digest:x}")
}

async fn thread_run(
    project_id: &str,
    thread_id: &str,
    services: &ProjectIntelligenceServices,
) -> Result<StatefulRun, FunctionCallError> {
    let run = services
        .runtime()
        .await
        .map_err(respond)?
        .run_for_thread(thread_id)
        .await
        .map_err(respond)?
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "the selected thread has no active Stateful run".to_string(),
            )
        })?;
    if run.value.project_id != project_id {
        return Err(FunctionCallError::RespondToModel(
            "the selected thread's run does not belong to the selected project".to_string(),
        ));
    }
    Ok(run)
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "roster_budget_tests.rs"]
mod roster_budget_tests;
