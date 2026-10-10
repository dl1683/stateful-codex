//! Core's side of host-ended answers.
//!
//! An extension may grant one answering task the right to end its run with the task's final
//! answer by inserting a [`HostAnswerReservation`] into the turn store before the task is
//! registered (see that type for the protocol). Core holds no run policy; this module only
//! connects the reservation to the task lifecycle:
//!
//! - task start: a task that does not answer with the model, or that starts while earlier
//!   thread-owned work (a user shell command, a background terminal, a code cell, a live
//!   agent, or any earlier executable hook) is outstanding, is disqualified;
//! - observation: every output item other than a message or reasoning, every failed or
//!   preempted response, compaction and elicitation disqualify, at first observation;
//! - hooks: the turn dispatches only its pinned hook set, and the set seen at each Stop
//!   dispatch must be empty;
//! - admission: user shell commands are refused while the reservation is live, and steering
//!   is refused once the task's input is closed;
//! - finalization: after the task's last model response the task closes its input under the
//!   active-turn lock and calls the owning extension while it is still registered;
//! - abort: an abort before commit authorization wins; after it, the abort waits for the
//!   task to end instead of cancelling it.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_extension_api::AbortArbitration;
use codex_extension_api::HostAnswerDisqualifier;
use codex_extension_api::HostAnswerPhase;
use codex_extension_api::HostAnswerReservation;
use codex_extension_api::TurnFinalizeOutcome;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use tokio::sync::watch;

use crate::CodexThread;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::RunningTask;

/// The message for auxiliary work refused while a run is answering.
pub(crate) const AUXILIARY_WORK_REFUSED: &str = "A Stateful run is answering in this thread's current turn, so no command can start beside it. Run it after the turn ends, or interrupt the turn first.";

/// The reservation held by `turn`, if an extension granted one.
pub(crate) fn reservation(turn: &TurnContext) -> Option<Arc<HostAnswerReservation>> {
    turn.extension_data.get::<HostAnswerReservation>()
}

pub(crate) fn disqualify(turn: &TurnContext, reason: HostAnswerDisqualifier) {
    if let Some(reservation) = reservation(turn) {
        reservation.disqualify(reason);
    }
}

/// Records one output item of a model response, at its first observation (added or done),
/// before it is routed, filtered or recorded.
pub(crate) fn observe_output_item(turn: &TurnContext, item: &ResponseItem) {
    let reason = match item {
        ResponseItem::Message { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::AgentMessage { .. } => return,
        ResponseItem::LocalShellCall { .. }
        | ResponseItem::FunctionCall { .. }
        | ResponseItem::ToolSearchCall { .. }
        | ResponseItem::CustomToolCall { .. }
        | ResponseItem::WebSearchCall { .. }
        | ResponseItem::ImageGenerationCall { .. } => HostAnswerDisqualifier::ToolCall,
        ResponseItem::AdditionalTools { .. }
        | ResponseItem::FunctionCallOutput { .. }
        | ResponseItem::CustomToolCallOutput { .. }
        | ResponseItem::ToolSearchOutput { .. }
        | ResponseItem::Compaction { .. }
        | ResponseItem::ConfigurationUpdate { .. }
        | ResponseItem::CompactionTrigger { .. }
        | ResponseItem::ContextCompaction { .. }
        | ResponseItem::Other => HostAnswerDisqualifier::UnrecognizedOutput,
    };
    disqualify(turn, reason);
}

/// Records the hook set a Stop dispatch is about to run for `turn`.
pub(crate) fn record_stop_hook_set(turn: &TurnContext, hooks: &codex_hooks::Hooks) {
    if let Some(reservation) = reservation(turn) {
        reservation.record_hook_set(hooks.is_empty());
    }
}

/// Marks that this thread has dispatched or published an executable hook set; asynchronous
/// hook jobs it started may outlive their turn.
pub(crate) struct ExecutableHooksSeen;

/// User shell commands admitted on this thread whose execution has not ended.
#[derive(Default)]
pub(crate) struct OutstandingUserShells(AtomicUsize);

/// Counts one admitted user shell command until it is dropped.
pub(crate) struct UserShellAdmission(Arc<OutstandingUserShells>);

impl Drop for UserShellAdmission {
    fn drop(&mut self) {
        self.0.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn admit_user_shell(sess: &Session) -> UserShellAdmission {
    let outstanding = sess
        .services
        .thread_extension_data
        .get_or_init(OutstandingUserShells::default);
    outstanding.0.fetch_add(1, Ordering::SeqCst);
    UserShellAdmission(outstanding)
}

/// Decides a task's reservation at registration: called under the active-turn lock, after
/// the turn's start callbacks and hook pinning, before the task is visible.
pub(crate) async fn check_task_start(
    sess: &Session,
    turn: &TurnContext,
    hooks: &codex_hooks::Hooks,
    answers_with_model: bool,
) {
    if !hooks.is_empty() {
        sess.services
            .thread_extension_data
            .insert(ExecutableHooksSeen);
    }
    let Some(reservation) = reservation(turn) else {
        return;
    };
    if !answers_with_model {
        reservation.disqualify(HostAnswerDisqualifier::NotAnsweringTask);
        return;
    }
    if !hooks.is_empty() {
        reservation.disqualify(HostAnswerDisqualifier::ExecutableHooks);
        return;
    }
    let outstanding_shells = sess
        .services
        .thread_extension_data
        .get::<OutstandingUserShells>()
        .is_some_and(|outstanding| outstanding.0.load(Ordering::SeqCst) > 0);
    if outstanding_shells
        || sess
            .services
            .thread_extension_data
            .get::<ExecutableHooksSeen>()
            .is_some()
        || !sess.list_background_terminals().await.is_empty()
        || sess.services.code_mode_service.has_active_cells()
        || !sess
            .services
            .agent_control
            .child_agent_paths(sess.thread_id)
            .await
            .is_empty()
    {
        reservation.disqualify(HostAnswerDisqualifier::OutstandingWork);
    }
}

/// Whether auxiliary work must be refused beside `task`.
pub(crate) fn refuses_auxiliary_work(task: &RunningTask) -> bool {
    reservation(&task.turn_context).is_some_and(|reservation| reservation.refuses_auxiliary_work())
}

/// Whether `task` closed its input: new input belongs to a later turn.
pub(crate) fn input_closed(task: &RunningTask) -> bool {
    reservation(&task.turn_context).is_some_and(|reservation| reservation.input_closed())
}

/// Arbitrates an abort of `task`. `Some` carries the task's end signal: a commit was
/// authorized, so the caller must wait for the task to end instead of aborting it.
pub(crate) fn arbitrate_abort(task: &RunningTask) -> Option<watch::Receiver<bool>> {
    let reservation = reservation(&task.turn_context)?;
    match reservation.arbitrate_abort() {
        AbortArbitration::Proceed => None,
        AbortArbitration::AwaitTaskEnd => Some(task.finished.clone()),
    }
}

/// Waits until a task whose commit won an abort race has ended.
pub(crate) async fn await_task_end(mut finished: watch::Receiver<bool>) {
    // A dropped sender also means the task ended.
    let _ = finished.wait_for(|finished| *finished).await;
}

/// Finalizes a regular task whose input is closed: lets the owning extension end the run
/// with `last_agent_message`. Returns `TurnAborted` when an abort arrived while a commit
/// was in flight and the commit did not become durable.
pub(crate) async fn finalize(
    sess: &Arc<Session>,
    turn: &TurnContext,
    last_agent_message: Option<&str>,
) -> CodexResult<()> {
    let Some(reservation) = reservation(turn) else {
        return Ok(());
    };
    if reservation.phase() != HostAnswerPhase::Finalizing {
        return Ok(());
    }
    if let Some(message) = last_agent_message.filter(|message| !message.trim().is_empty()) {
        for contributor in sess.services.extensions.turn_lifecycle_contributors() {
            let outcome = contributor
                .on_turn_finalize(codex_extension_api::TurnFinalizeInput {
                    reservation: reservation.as_ref(),
                    last_agent_message: message,
                    session_store: &sess.services.session_extension_data,
                    thread_store: &sess.services.thread_extension_data,
                    turn_store: turn.extension_data.as_ref(),
                })
                .await;
            match outcome {
                TurnFinalizeOutcome::NotHandled => continue,
                TurnFinalizeOutcome::Committed => {}
                TurnFinalizeOutcome::Declined(reason) => {
                    tracing::debug!(turn_id = %turn.sub_id, %reason, "host-ended answer declined");
                }
                TurnFinalizeOutcome::Failed(message) => {
                    sess.send_event(turn, EventMsg::Warning(WarningEvent { message }))
                        .await;
                }
            }
            break;
        }
    }
    let phase = reservation.end_finalization();
    if phase == HostAnswerPhase::NotCommitted && reservation.abort_requested() {
        return Err(CodexErr::TurnAborted);
    }
    Ok(())
}

/// Closes a finished regular task's input under the active-turn lock. Returns false when
/// input is pending, so the task must run another model turn first; otherwise any
/// reservation the task holds enters finalization (or stays an ordinary task).
#[expect(
    clippy::await_holding_invalid_type,
    reason = "the pending-input check and the input closure must be atomic with steering"
)]
pub(crate) async fn close_input(sess: &Session, turn: &TurnContext) -> bool {
    let active = sess.active_turn.lock().await;
    let Some(active_turn) = active.as_ref() else {
        return !sess.input_queue.has_pending_mailbox_items().await;
    };
    let (has_turn_pending_input, accepts_mailbox_delivery) = {
        let turn_state = active_turn.turn_state.lock().await;
        (
            !turn_state.pending_input.is_empty(),
            turn_state.accepts_mailbox_delivery_for_current_turn(),
        )
    };
    if accepts_mailbox_delivery
        && (has_turn_pending_input || sess.input_queue.has_pending_mailbox_items().await)
    {
        return false;
    }
    if let Some(reservation) = reservation(turn)
        && active_turn
            .task
            .as_ref()
            .is_some_and(|task| task.turn_context.sub_id == turn.sub_id)
    {
        reservation.begin_finalization();
    }
    true
}

impl CodexThread {
    /// Whether this thread's active turn holds a live host-answer reservation, so work beside
    /// it (a detached review started from this thread) must be refused.
    pub async fn refuses_auxiliary_work(&self) -> bool {
        let active = self.session.active_turn.lock().await;
        active
            .as_ref()
            .and_then(|turn| turn.task.as_ref())
            .is_some_and(refuses_auxiliary_work)
    }
}
