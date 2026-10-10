//! Delivery of tool results whose runtime attached a [`ToolPublicationCheck`].
//!
//! Such a result, and the PostToolUse context derived from it, stays pending until the
//! sampling loop records it into model history. Only there is the check run, and its guard
//! is held just for that recording: never across a model request, a stream wait, a hook or
//! another tool. A runtime can so order the actual delivery against its own writers. A
//! withheld result is recorded as the runtime's bounded replacement text and its hook
//! context is dropped. Results without a check are recorded exactly as before.

use std::sync::Arc;

use codex_history::ResponseItemEnvelope;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
use codex_tools::ToolPublicationCheck;

use crate::hook_runtime::record_additional_contexts;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolCallState;

/// A checked result's publication, carried with the call until it is recorded.
pub(crate) struct PendingPublication {
    check: ToolPublicationCheck,
    hook_contexts: Vec<String>,
}

/// One tool call's model-visible response and its pending publication, if any.
pub(crate) struct ToolDelivery {
    pub(crate) envelope: ResponseItemEnvelope,
    pub(crate) publication: Option<PendingPublication>,
}

/// Keeps `check` and the hook context derived from the same result pending with the call.
/// Returns the withheld text when the result cannot reach a direct delivery boundary
/// (a nested Code Mode call, or a call without host state): it is then withheld at once.
pub(crate) fn defer(
    check: ToolPublicationCheck,
    hook_contexts: Vec<String>,
    source: &ToolCallSource,
    call_state: Option<&ToolCallState>,
) -> Option<String> {
    match (source, call_state) {
        (ToolCallSource::Direct | ToolCallSource::DirectPlaintextMessage, Some(state)) => {
            *state
                .publication
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(PendingPublication {
                check,
                hook_contexts,
            });
            None
        }
        (ToolCallSource::CodeMode { .. }, _) | (_, None) => Some(check.withheld),
    }
}

/// The pending publication recorded for a finished call.
pub(crate) fn take(call_state: &ToolCallState) -> Option<PendingPublication> {
    call_state
        .publication
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Records one tool response into model history, running its publication check first and
/// holding the check's guard until the response and its hook context are recorded.
pub(crate) async fn record(
    sess: &Arc<Session>,
    step_context: &StepContext,
    delivery: ToolDelivery,
) {
    let turn_context = &step_context.turn;
    let model_info = &step_context.settings.model_info;
    let ToolDelivery {
        mut envelope,
        publication,
    } = delivery;
    let Some(PendingPublication {
        check,
        hook_contexts,
    }) = publication
    else {
        sess.record_annotated_conversation_items(turn_context, model_info, vec![envelope])
            .await;
        return;
    };
    let guard = (check.validate)().await;
    if guard.is_none() {
        withhold(&mut envelope.item, check.withheld);
    }
    sess.record_annotated_conversation_items(turn_context, model_info, vec![envelope])
        .await;
    if guard.is_some() {
        record_additional_contexts(sess, turn_context, hook_contexts).await;
    }
    drop(guard);
}

/// Replaces a tool output's model-visible content (and any result metadata) with `text`.
fn withhold(item: &mut ResponseItem, text: String) {
    match item {
        ResponseItem::FunctionCallOutput { output, .. }
        | ResponseItem::CustomToolCallOutput { output, .. } => {
            *output = FunctionCallOutputPayload::from_text(text);
            item.clear_tool_result_metadata();
        }
        // Checks come only from extension function tools, whose responses are function or
        // custom tool outputs; no other item is a checked tool result.
        ResponseItem::Message { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::AgentMessage { .. }
        | ResponseItem::AdditionalTools { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::FunctionCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::ConfigurationUpdate { .. }
        | ResponseItem::CompactionTrigger { .. }
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => {}
    }
}
