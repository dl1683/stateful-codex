//! Host fences behind the no-tool exemption: every action of a run is durably recorded
//! against it before the action can run, so the completion decision reads host facts rather
//! than the model's account.
//!
//! - Model output: every model response passes through `HostActionObserver` before Core sees
//!   it. Before the first tool call of a response (function, custom, local shell, tool search,
//!   hosted web search or image generation, or any other non-text item) reaches Core, the
//!   observer records one action for the thread's open runs. A response whose only call is a
//!   `stateful_run_update` completion is the one exception: that call is held back until the
//!   response completes, then released unrecorded if no other call appeared, or recorded first
//!   if one did, in either order. If the record cannot be written, the response fails instead
//!   and nothing in it is dispatched.
//! - User shell commands are recorded by the tool policy before they are spawned, and refused
//!   if the record cannot be written.
//! - Lifecycle hooks run around calls and turns; a completion in a process where hooks were
//!   configured records an action first (`tools::run_acceptance`).

use std::collections::VecDeque;

use codex_extension_api::ModelRequestContributor;
use codex_extension_api::ModelRequestInput;
use codex_extension_api::ModelRequestKind;
use codex_extension_api::ModelResponseError;
use codex_extension_api::ModelResponseInterceptor;
use codex_extension_api::ModelResponseStream;
use codex_extension_api::ResponseEvent;
use codex_protocol::models::ResponseItem;
use futures::StreamExt;

use crate::services::ProjectIntelligenceServices;

/// The run's own completion tool, the only call a no-tool run may make.
const COMPLETION_TOOL: &str = "stateful_run_update";

type Event = Result<ResponseEvent, ModelResponseError>;

/// Fences every generation response of a Stateful host behind the run's action record.
pub(crate) struct HostActionObserver {
    pub(crate) services: ProjectIntelligenceServices,
}

impl std::fmt::Debug for HostActionObserver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostActionObserver")
            .finish_non_exhaustive()
    }
}

impl ModelRequestContributor for HostActionObserver {
    fn request(&self, input: ModelRequestInput<'_>) -> Option<Box<dyn ModelResponseInterceptor>> {
        match input.kind {
            ModelRequestKind::Generation => Some(Box::new(ResponseFence {
                services: self.services.clone(),
                thread_id: input.thread_id.to_string(),
            })),
            ModelRequestKind::Warmup => None,
        }
    }
}

struct ResponseFence {
    services: ProjectIntelligenceServices,
    thread_id: String,
}

impl ModelResponseInterceptor for ResponseFence {
    fn intercept(self: Box<Self>, stream: ModelResponseStream) -> ModelResponseStream {
        let fence = FenceState {
            upstream: stream,
            services: self.services,
            thread_id: self.thread_id,
            recorded: false,
            held: None,
            held_behind_other_call: false,
            ready: VecDeque::new(),
            finished: false,
        };
        Box::pin(futures::stream::unfold(fence, FenceState::next))
    }
}

/// What one completed output item means for the run's action record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputKind {
    /// Text the model produced; no action.
    Text,
    /// A `stateful_run_update` call completing the run.
    Completion,
    /// Any other call or non-text item.
    Call,
}

fn classify(item: &ResponseItem) -> OutputKind {
    match item {
        ResponseItem::Message { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::AgentMessage { .. } => OutputKind::Text,
        ResponseItem::FunctionCall {
            name,
            namespace: None,
            arguments,
            encrypted_function_args,
            ..
        } if name == COMPLETION_TOOL
            && encrypted_function_args
                .as_ref()
                .is_none_or(std::vec::Vec::is_empty)
            && serde_json::from_str::<serde_json::Value>(arguments)
                .is_ok_and(|arguments| arguments["status"] == "completed") =>
        {
            OutputKind::Completion
        }
        ResponseItem::FunctionCall { .. }
        | ResponseItem::AdditionalTools { .. }
        | ResponseItem::LocalShellCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::ConfigurationUpdate { .. }
        | ResponseItem::CompactionTrigger {}
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => OutputKind::Call,
    }
}

struct FenceState {
    upstream: ModelResponseStream,
    services: ProjectIntelligenceServices,
    thread_id: String,
    /// This response's action is durably recorded; later calls pass straight through.
    recorded: bool,
    /// Events held behind a lone completion call until the response completes.
    held: Option<Vec<Event>>,
    held_behind_other_call: bool,
    ready: VecDeque<Event>,
    finished: bool,
}

impl FenceState {
    async fn next(mut self) -> Option<(Event, Self)> {
        loop {
            if let Some(event) = self.ready.pop_front() {
                return Some((event, self));
            }
            if self.finished {
                return None;
            }
            match self.upstream.next().await {
                Some(event) => self.accept(event).await,
                None => {
                    // The response ended without completing: a held completion is never
                    // released, so it cannot run.
                    self.held = None;
                    self.finished = true;
                }
            }
        }
    }

    async fn accept(&mut self, event: Event) {
        let kind = match &event {
            Ok(ResponseEvent::OutputItemDone(item)) => Some(classify(item)),
            _ => None,
        };
        if let Some(held) = self.held.as_mut() {
            match event {
                Ok(ResponseEvent::Completed { .. }) => {
                    let mut events = self.held.take().unwrap_or_default();
                    events.push(event);
                    if self.held_behind_other_call && !self.record().await {
                        return;
                    }
                    self.ready.extend(events);
                }
                // A failed response never releases its held completion.
                Err(_) => {
                    self.held = None;
                    self.ready.push_back(event);
                }
                Ok(_) => {
                    if matches!(kind, Some(OutputKind::Completion | OutputKind::Call)) {
                        self.held_behind_other_call = true;
                    }
                    held.push(event);
                }
            }
            return;
        }
        match kind {
            Some(OutputKind::Completion) if !self.recorded => self.held = Some(vec![event]),
            Some(OutputKind::Completion | OutputKind::Call) => {
                if self.record().await {
                    self.ready.push_back(event);
                }
            }
            Some(OutputKind::Text) | None => self.ready.push_back(event),
        }
    }

    /// Records this response's action once. On failure the response fails in its place, so
    /// nothing after this point reaches Core.
    async fn record(&mut self) -> bool {
        if self.recorded {
            return true;
        }
        let recorded = match self.services.runtime().await {
            Ok(store) => store.record_host_action_for_thread(&self.thread_id).await,
            Err(error) => Err(error),
        };
        match recorded {
            Ok(()) => {
                self.recorded = true;
                true
            }
            Err(error) => {
                tracing::warn!(thread_id = %self.thread_id, %error, "failed to record a model tool call before dispatch");
                self.ready.push_back(Err(ModelResponseError::Stream(format!(
                    "the host could not durably record this response's tool calls before running them ({error}); none of them ran"
                ))));
                self.finished = true;
                false
            }
        }
    }
}

/// Records a user shell command against the thread's open runs before it is spawned.
pub(crate) async fn record_user_shell(
    services: &ProjectIntelligenceServices,
    thread_id: &str,
) -> Result<(), codex_stateful_runtime::StatefulRunStoreError> {
    services
        .runtime()
        .await?
        .record_host_action_for_thread(thread_id)
        .await
}

#[cfg(test)]
#[path = "host_actions_tests.rs"]
mod tests;
