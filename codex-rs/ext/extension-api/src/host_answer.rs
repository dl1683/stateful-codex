//! Host-ended answer reservation shared by Core and the extension that owns a run.
//!
//! An extension that grants one answering task the right to end its run with that task's
//! final answer inserts one [`HostAnswerReservation`] into the turn store before the task is
//! registered. From then on the reservation is the single place where Core and the extension
//! agree on what happened to that task:
//!
//! - Core records disqualifying observations as they happen: any observed tool call or
//!   unrecognized output item, a failed or preempted response, compaction, an executable
//!   hook policy, outstanding thread-owned work, an elicitation, or a task that is not a
//!   model-answering task. Disqualification is sticky; nothing clears it.
//! - Core refuses auxiliary work (user shell commands, detached reviews) while the
//!   reservation is live, so no command can be admitted beside a candidate answer.
//! - Core closes the task's input and enters finalization only after the task's last model
//!   response, while the task is still registered and interruptible.
//! - The extension authorizes its terminal write immediately before the durable commit and
//!   resolves it with the durable outcome.
//! - Core arbitrates every abort against that commit: an abort that arrives before
//!   authorization wins and the commit is refused; an abort that arrives after it waits for
//!   the task to end, so a completed answer is never reported as cancelled.

use std::sync::Mutex;
use std::sync::PoisonError;

/// Why a reservation can no longer end its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostAnswerDisqualifier {
    /// The model emitted a tool call of any kind (observed when added or done).
    ToolCall,
    /// The model emitted an output item that is neither a message nor reasoning.
    UnrecognizedOutput,
    /// A model response failed, was retried, or was preempted.
    ResponseFailed,
    /// The turn compacted its history.
    Compaction,
    /// The turn's hook policy could run a hook.
    ExecutableHooks,
    /// The turn's hook policy was never confirmed empty.
    HooksUnverified,
    /// Earlier thread-owned work (a shell command, terminal, code cell or agent) was still
    /// outstanding when the task started.
    OutstandingWork,
    /// An elicitation was requested during the turn.
    Elicitation,
    /// The task is not a model-answering task.
    NotAnsweringTask,
    /// The turn ended with an error, without a final answer, or was cancelled.
    TurnFailed,
}

/// Where a reservation stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostAnswerPhase {
    /// The task is answering; auxiliary work is refused.
    Answering,
    /// The reservation can no longer end the run; the task continues as an ordinary task.
    Disqualified(HostAnswerDisqualifier),
    /// The task's input is closed and the owning extension is deciding.
    Finalizing,
    /// The extension authorized its terminal commit; the durable outcome is pending.
    Committing,
    /// The terminal commit is durable.
    Committed,
    /// Finalization ended without a durable commit.
    NotCommitted,
    /// An abort won before any commit was authorized.
    Aborted,
}

/// How an abort must treat a task that holds a reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortArbitration {
    /// No commit was authorized; abort the task normally. The reservation is now aborted.
    Proceed,
    /// A commit was authorized; wait for the task to end on its own instead of aborting it.
    AwaitTaskEnd,
}

#[derive(Debug)]
struct State {
    phase: HostAnswerPhase,
    hooks_confirmed_empty: bool,
    abort_requested: bool,
}

/// The single-use right of one answering task to end its run with its final answer.
#[derive(Debug)]
pub struct HostAnswerReservation {
    run_id: String,
    thread_id: String,
    turn_id: String,
    state: Mutex<State>,
}

impl HostAnswerReservation {
    pub fn new(run_id: String, thread_id: String, turn_id: String) -> Self {
        Self {
            run_id,
            thread_id,
            turn_id,
            state: Mutex::new(State {
                phase: HostAnswerPhase::Answering,
                hooks_confirmed_empty: false,
                abort_requested: false,
            }),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn phase(&self) -> HostAnswerPhase {
        self.state().phase
    }

    /// Records a disqualifying observation. Only an answering reservation changes.
    pub fn disqualify(&self, reason: HostAnswerDisqualifier) {
        let mut state = self.state();
        if state.phase == HostAnswerPhase::Answering {
            state.phase = HostAnswerPhase::Disqualified(reason);
        }
    }

    /// Records the hook set about to be dispatched for the task: any hook disqualifies, and
    /// finalization requires at least one dispatch site to have seen an empty set.
    pub fn record_hook_set(&self, hooks_are_empty: bool) {
        let mut state = self.state();
        if !hooks_are_empty {
            if state.phase == HostAnswerPhase::Answering {
                state.phase =
                    HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ExecutableHooks);
            }
            return;
        }
        state.hooks_confirmed_empty = true;
    }

    /// Whether auxiliary work (a user shell command, a detached review) must be refused now.
    pub fn refuses_auxiliary_work(&self) -> bool {
        matches!(
            self.state().phase,
            HostAnswerPhase::Answering
                | HostAnswerPhase::Finalizing
                | HostAnswerPhase::Committing
                | HostAnswerPhase::Committed
        )
    }

    /// Whether the task's input is closed: new input belongs to a later turn.
    pub fn input_closed(&self) -> bool {
        matches!(
            self.state().phase,
            HostAnswerPhase::Finalizing | HostAnswerPhase::Committing | HostAnswerPhase::Committed
        )
    }

    /// Closes the task's input and enters finalization. Returns false, leaving the task an
    /// ordinary task, when the reservation was disqualified or aborted, or when no dispatch
    /// site confirmed an empty hook set.
    pub fn begin_finalization(&self) -> bool {
        let mut state = self.state();
        if state.phase != HostAnswerPhase::Answering {
            return false;
        }
        if !state.hooks_confirmed_empty {
            state.phase = HostAnswerPhase::Disqualified(HostAnswerDisqualifier::HooksUnverified);
            return false;
        }
        state.phase = HostAnswerPhase::Finalizing;
        true
    }

    /// Authorizes the terminal commit. Call immediately before the durable commit, after
    /// every check; false means an abort won and the commit must be rolled back.
    pub fn authorize_commit(&self) -> bool {
        let mut state = self.state();
        if state.phase != HostAnswerPhase::Finalizing {
            return false;
        }
        state.phase = HostAnswerPhase::Committing;
        true
    }

    /// Resolves an authorized commit with its durable outcome.
    pub fn resolve_commit(&self, committed: bool) {
        let mut state = self.state();
        if state.phase == HostAnswerPhase::Committing {
            state.phase = if committed {
                HostAnswerPhase::Committed
            } else {
                HostAnswerPhase::NotCommitted
            };
        }
    }

    /// Ends finalization: a reservation still finalizing or committing is marked not
    /// committed, so auxiliary work and aborts proceed normally again.
    pub fn end_finalization(&self) -> HostAnswerPhase {
        let mut state = self.state();
        if matches!(
            state.phase,
            HostAnswerPhase::Finalizing | HostAnswerPhase::Committing
        ) {
            state.phase = HostAnswerPhase::NotCommitted;
        }
        state.phase
    }

    /// Arbitrates an abort of the task against its terminal commit.
    pub fn arbitrate_abort(&self) -> AbortArbitration {
        let mut state = self.state();
        match state.phase {
            HostAnswerPhase::Answering
            | HostAnswerPhase::Disqualified(_)
            | HostAnswerPhase::Finalizing => {
                state.phase = HostAnswerPhase::Aborted;
                AbortArbitration::Proceed
            }
            HostAnswerPhase::Committing => {
                state.abort_requested = true;
                AbortArbitration::AwaitTaskEnd
            }
            HostAnswerPhase::Committed => AbortArbitration::AwaitTaskEnd,
            HostAnswerPhase::NotCommitted | HostAnswerPhase::Aborted => AbortArbitration::Proceed,
        }
    }

    /// Whether an abort arrived while a commit was in flight.
    pub fn abort_requested(&self) -> bool {
        self.state().abort_requested
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
