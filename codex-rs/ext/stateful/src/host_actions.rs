//! Host fences behind the no-tool exemption: every action of a run is durably recorded
//! against it before the action can run, so the completion decision reads host facts rather
//! than the model's account.
//!
//! - Model output: every model response passes through `HostActionObserver` before Core sees
//!   it. Every call item (function, custom, local shell, tool search, hosted web search or
//!   image generation, or any other non-text item) is recorded against the thread's open runs
//!   as soon as it is first observed, added or done, before that event passes on; each record
//!   uses the run bindings current at that moment. A `stateful_run_update` completion is the
//!   one exception: it is held, with the events after it, until the response completes; then
//!   one completion attempt is recorded and it is released. A call after it proves the
//!   response is not action-free: the call is recorded and everything held flows at once. If
//!   a record cannot be written, the response fails instead and nothing after it reaches Core.
//!   The held events are bounded in count and bytes; past either bound the response fails and
//!   the completion is never released.
//! - Provider-hosted calls (web search and the like) run at the provider, a read with no local
//!   effect; each counts as an action once the host observes it, recorded like any other call
//!   before a held completion can be released. A hosted call the provider never reports is not
//!   accounted.
//! - Whenever a record failed or was abandoned mid-write, an observed call may have gone
//!   unrecorded, so the process notes it; every later completion in it then records an action
//!   first (`tools::run_acceptance`), so none is exempt.
//! - User shell commands are recorded by the tool policy before they are spawned, and refused
//!   if the record cannot be written.
//! - Lifecycle hooks run around calls and turns; a completion in a process where hooks were
//!   configured records an action first (`tools::run_acceptance`).

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

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
/// Most events held behind a completion while its response may still carry other calls.
const MAX_HELD_EVENTS: usize = 1_024;
/// Most bytes (as rendered for debugging) held behind a completion.
const MAX_HELD_BYTES: usize = 1024 * 1024;

type Event = Result<ResponseEvent, ModelResponseError>;

/// Set once this process may have let an action run that no run recorded.
static UNRECORDED_ACTIONS_POSSIBLE: AtomicBool = AtomicBool::new(false);

/// Whether an observed call may have gone unrecorded in this process: a record failed or was
/// abandoned. Sticky for the life of the process.
pub(crate) fn unrecorded_actions_possible() -> bool {
    UNRECORDED_ACTIONS_POSSIBLE.load(Ordering::SeqCst)
}

fn note_unrecorded_actions_possible() {
    UNRECORDED_ACTIONS_POSSIBLE.store(true, Ordering::SeqCst);
}

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
            called: false,
            held: None,
            held_bytes: 0,
            ready: VecDeque::new(),
            finished: false,
            recording: false,
        };
        Box::pin(futures::stream::unfold(fence, FenceState::next))
    }
}

/// What one observed output item means for the run's action record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputKind {
    /// Text the model produced, or a completion call still being streamed; no action yet.
    Text,
    /// A `stateful_run_update` call completing the run.
    Completion,
    /// Any other call or non-text item.
    Call,
}

fn observe(event: &Event) -> OutputKind {
    match event {
        Ok(ResponseEvent::OutputItemDone(item)) => classify(item),
        Ok(ResponseEvent::OutputItemAdded(item)) => match classify(item) {
            // Its arguments are judged when it is done; until then it cannot be dispatched.
            OutputKind::Completion => OutputKind::Text,
            OutputKind::Call if is_completion_name(item) => OutputKind::Text,
            kind @ (OutputKind::Text | OutputKind::Call) => kind,
        },
        Ok(_) | Err(_) => OutputKind::Text,
    }
}

fn is_completion_name(item: &ResponseItem) -> bool {
    matches!(
        item,
        ResponseItem::FunctionCall { name, namespace: None, .. } if name == COMPLETION_TOOL
    )
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

/// What a record counts.
#[derive(Clone, Copy)]
enum Record {
    Action,
    CompletionAttempt,
}

struct FenceState {
    upstream: ModelResponseStream,
    services: ProjectIntelligenceServices,
    thread_id: String,
    /// A call of this response was recorded, so a completion in it is not action-free.
    called: bool,
    /// A completion and the events after it, held until the response completes.
    held: Option<Vec<Event>>,
    held_bytes: usize,
    ready: VecDeque<Event>,
    finished: bool,
    /// A record is being written; abandoning it leaves its outcome unknown.
    recording: bool,
}

impl Drop for FenceState {
    fn drop(&mut self) {
        if self.recording {
            note_unrecorded_actions_possible();
        }
    }
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
                // The response ended without completing: a held completion is never released,
                // so it cannot run.
                None => self.finish(),
            }
        }
    }

    async fn accept(&mut self, event: Event) {
        let kind = observe(&event);
        let Some(held) = self.held.as_mut() else {
            match kind {
                OutputKind::Completion if !self.called => {
                    self.held_bytes = held_size(&event);
                    self.held = Some(vec![event]);
                }
                OutputKind::Completion | OutputKind::Call => {
                    if self.record(Record::Action).await {
                        self.called = true;
                        self.ready.push_back(event);
                    }
                }
                OutputKind::Text => self.ready.push_back(event),
            }
            return;
        };
        match (&event, kind) {
            (Ok(ResponseEvent::Completed { .. }), _) => {
                held.push(event);
                if self.record(Record::CompletionAttempt).await {
                    self.release();
                }
            }
            // A failed response never releases its held completion.
            (Err(_), _) => {
                self.held = None;
                self.ready.push_back(event);
                self.finish();
            }
            // Another call proves the response is not action-free: record it, then let
            // everything held flow without further buffering.
            (Ok(_), OutputKind::Completion | OutputKind::Call) => {
                held.push(event);
                if self.record(Record::Action).await {
                    self.called = true;
                    self.release();
                }
            }
            (Ok(_), OutputKind::Text) => {
                self.held_bytes = self.held_bytes.saturating_add(held_size(&event));
                if held.len() >= MAX_HELD_EVENTS || self.held_bytes > MAX_HELD_BYTES {
                    self.fail(format!(
                        "the response kept streaming past the host's bound ({MAX_HELD_EVENTS} events or {MAX_HELD_BYTES} bytes) while a completion call waited for it to finish; the completion did not run"
                    ));
                } else {
                    held.push(event);
                }
            }
        }
    }

    fn release(&mut self) {
        self.ready.extend(self.held.take().unwrap_or_default());
        self.held_bytes = 0;
    }

    /// Ends the response with `message` in place of anything not yet passed on.
    fn fail(&mut self, message: String) {
        self.ready
            .push_back(Err(ModelResponseError::Stream(message)));
        self.finish();
    }

    /// Stops reading: anything held is discarded and the upstream response is released.
    fn finish(&mut self) {
        self.held = None;
        self.held_bytes = 0;
        self.finished = true;
        self.upstream = Box::pin(futures::stream::empty());
    }

    /// Records against the thread's runs bound now. On failure the response fails in place of
    /// the event, so nothing after this point reaches Core.
    async fn record(&mut self, record: Record) -> bool {
        self.recording = true;
        let recorded = match self.services.runtime().await {
            Ok(store) => match record {
                Record::Action => store.record_host_action_for_thread(&self.thread_id).await,
                Record::CompletionAttempt => {
                    store
                        .record_completion_attempt_for_thread(&self.thread_id)
                        .await
                }
            },
            Err(error) => Err(error),
        };
        self.recording = false;
        match recorded {
            Ok(()) => true,
            Err(error) => {
                note_unrecorded_actions_possible();
                tracing::warn!(thread_id = %self.thread_id, %error, "failed to record a model call before dispatch");
                self.fail(format!(
                    "the host could not durably record this response's tool calls before running them ({error}); none of them ran"
                ));
                false
            }
        }
    }
}

/// The size an event holds while buffered, as rendered for debugging (at least its text).
fn held_size(event: &Event) -> usize {
    format!("{event:?}").len()
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
