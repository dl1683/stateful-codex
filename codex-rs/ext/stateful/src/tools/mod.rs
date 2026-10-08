mod batch_input;
mod blackboard;
mod blackboard_evidence;
mod blackboard_premises;
mod blackboard_supersede;
mod blackboard_update;
mod blackboard_write;
mod context_map;
mod conversation_read;
mod entry_read;
mod evidence;
mod memory_read;
mod obligation;
mod run;
mod run_read;
mod source_proposals;
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
/// Serialized bound for a decoding diagnostic echoed to the model. Serde diagnostics can
/// quote arbitrary input, such as a huge unknown key; a longer one is replaced by a
/// generic statement.
const MAX_DECODE_ERROR_BYTES: usize = 640;

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
            visible_root.clone(),
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
            services.clone(),
        )),
        Arc::new(memory_read::MemoryReadTool::new(
            project_id.clone(),
            services.clone(),
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

/// Decodes a tool's arguments. A rejection names the offending field path and what it
/// accepts, then shows `example`, a minimal valid call, so a model can correct the call
/// in one retry. Both parts are dropped, in that order, when the call's response
/// allowance cannot hold them.
fn parse_arguments<T: for<'de> serde::Deserialize<'de>>(
    call: &ToolCall<'_>,
    example: &str,
) -> Result<T, codex_extension_api::FunctionCallError> {
    let arguments = call.function_arguments()?;
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    let mut deserializer = serde_json::Deserializer::from_str(arguments);
    let decoded = match serde_path_to_error::deserialize(&mut deserializer) {
        Ok(value) => deserializer
            .end()
            .map(|()| value)
            .map_err(|error| error.to_string()),
        Err(error) => Err(match error.path().to_string().as_str() {
            "." => error.inner().to_string(),
            path => format!("field `{path}`: {}", error.inner()),
        }),
    };
    decoded.map_err(|message| {
        // Account for JSON escaping as well as the actual tool's response allowance.
        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let detail = if serde_json::to_string(&message)
            .is_ok_and(|text| text.len() <= budget.min(MAX_DECODE_ERROR_BYTES))
        {
            message
        } else {
            "invalid tool arguments".to_string()
        };
        let guided = format!("{detail}. Minimal valid call: {example}");
        if serde_json::to_string(&guided).is_ok_and(|text| text.len() <= budget) {
            FunctionCallError::RespondToModel(guided)
        } else {
            bounded_respond(call, &detail)
        }
    })
}

/// Fits a terminal diagnostic, including JSON escaping, to the call's allowance.
fn bounded_respond(call: &ToolCall<'_>, message: &str) -> FunctionCallError {
    let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
    let mut message = if serde_json::to_string(message).is_ok_and(|text| text.len() <= budget) {
        message.to_string()
    } else {
        "budget_insufficient".to_string()
    };
    while serde_json::to_string(&message).is_ok_and(|text| text.len() > budget)
        && !message.is_empty()
    {
        message.pop();
    }
    FunctionCallError::RespondToModel(message)
}

/// Emits a JSON result only when its serialized text fits the call's budget. Callers
/// that mutate state must size their output before mutating; this is the backstop.
fn bounded_json_output(
    call: &ToolCall<'_>,
    value: serde_json::Value,
) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
    let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
    if value.to_string().len() > budget {
        return Err(bounded_respond(
            call,
            &format!(
                "tool result withheld: it exceeded the {budget}-byte model item bound. Any change this call made was applied; query the affected state instead of retrying."
            ),
        ));
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
        return Err(bounded_respond(
            call,
            &format!(
                "nothing was written: this batch's receipts might not fit one {budget}-byte result. Split it into smaller batches."
            ),
        ));
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

/// Longest tool call ID kept verbatim as a provenance source. Some provider bridges carry
/// opaque reasoning state inside call IDs (LiteLLM appends Gemini thought signatures as
/// `call_<id>__thought__<signature>`), which exceeds every stored identity bound.
const MAX_VERBATIM_SOURCE_BYTES: usize = 128;

/// The provenance source recorded for one tool call: the call ID itself when it is short,
/// trimmed, and control-free, otherwise a stable digest of it. Either form names exactly
/// one bounded source, so stored provenance validation is unchanged.
fn provenance_source_id(call_id: &str) -> String {
    if !call_id.is_empty()
        && call_id.len() <= MAX_VERBATIM_SOURCE_BYTES
        && call_id.trim() == call_id
        && !call_id.chars().any(char::is_control)
    {
        return call_id.to_string();
    }
    format!("call-sha256:{:x}", Sha256::digest(call_id.as_bytes()))
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

#[cfg(test)]
#[path = "provider_call_id_tests.rs"]
mod provider_call_id_tests;

#[cfg(test)]
#[path = "schema_portability_tests.rs"]
mod schema_portability_tests;

#[cfg(test)]
#[path = "capture_repair_tests.rs"]
mod capture_repair_tests;

#[cfg(test)]
#[path = "capture_repair2_tests.rs"]
mod capture_repair2_tests;
