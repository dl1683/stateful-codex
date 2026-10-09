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
//!   Every event held, the completion itself and the event that releases it included, is
//!   admitted only within the count and byte bounds; past either the response fails, nothing
//!   held is released, no attempt is recorded and the upstream response is dropped. A call
//!   is accounted before its admission is decided, so overflow never drops an observed call
//!   unrecorded.
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
//!   configured records an action first (`tools::run_acceptance`). A turn's hook set is fixed
//!   when its task starts, and publishing hooks records that fact first, so a hook installed
//!   after the completion read the fact cannot run around it or later in its turn.

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
/// Most bytes (as rendered for debugging) held behind a completion, the completion included.
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
        if self.held.is_none() {
            match kind {
                OutputKind::Completion if !self.called => {
                    self.hold(event);
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
        }
        match (&event, kind) {
            (Ok(ResponseEvent::Completed { .. }), _) => {
                if self.hold(event) && self.record(Record::CompletionAttempt).await {
                    self.release();
                }
            }
            // A failed response never releases its held completion.
            (Err(_), _) => {
                self.held = None;
                self.ready.push_back(event);
                self.finish();
            }
            // Another call proves the response is not action-free. It is accounted before any
            // admission decision (a hosted call may already have run at the provider), then
            // everything held flows without further buffering. A call that does not fit is
            // dropped unretained once recorded, and the response fails.
            (Ok(_), OutputKind::Completion | OutputKind::Call) => {
                let admitted = self.admission(&event).map(|size| (event, size));
                if !self.record(Record::Action).await {
                    return;
                }
                self.called = true;
                match admitted {
                    Some((event, size)) => {
                        self.retain(event, size);
                        self.release();
                    }
                    None => self.overflow(),
                }
            }
            (Ok(_), OutputKind::Text) => {
                self.hold(event);
            }
        }
    }

    /// Holds `event` behind the completion if it fits both bounds; otherwise the response fails
    /// and nothing held is released.
    fn hold(&mut self, event: Event) -> bool {
        match self.admission(&event) {
            Some(size) => {
                self.retain(event, size);
                true
            }
            None => {
                self.overflow();
                false
            }
        }
    }

    /// The size `event` would take if it fits both bounds.
    fn admission(&self, event: &Event) -> Option<usize> {
        let held = self.held.as_ref().map_or(0, Vec::len);
        let room = MAX_HELD_BYTES - self.held_bytes;
        let size = held_size(event, room);
        (held < MAX_HELD_EVENTS && size <= room).then_some(size)
    }

    fn retain(&mut self, event: Event, size: usize) {
        self.held_bytes += size;
        self.held.get_or_insert_with(Vec::new).push(event);
    }

    fn overflow(&mut self) {
        self.fail(format!(
            "the response exceeded the host's bound ({MAX_HELD_EVENTS} events or {MAX_HELD_BYTES} bytes) held behind a completion call while it waited for the response to finish; the completion did not run"
        ));
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
                    "the host could not durably record a model call before dispatch ({error}); this call and the following local calls were not dispatched; earlier or provider-hosted actions may already have run"
                ));
                false
            }
        }
    }
}

/// The size an event holds while buffered, as rendered for debugging (at least its text).
/// Counting stops once it passes `limit`, without copying the event, so a result above
/// `limit` only says the event does not fit.
fn held_size(event: &Event, limit: usize) -> usize {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::fmt::Write for Counter {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.bytes = self.bytes.saturating_add(text.len());
            if self.bytes > self.limit {
                Err(std::fmt::Error)
            } else {
                Ok(())
            }
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    // An error only means the count passed `limit`.
    let _ = std::fmt::write(&mut counter, format_args!("{event:?}"));
    counter.bytes
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
