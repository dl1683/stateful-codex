//! Task-owned turn finalization.
//!
//! After a regular task's last model response, while the task is still registered and
//! interruptible, extensions may record a terminal outcome for the turn (a Stateful run that
//! ends with the turn's answer, for example) through
//! [`codex_extension_api::TurnLifecycleContributor::on_turn_finalize`].
//!
//! A per-turn commit gate orders that durable write against aborts of the same task:
//!
//! - an abort that arrives before the extension authorized its commit wins: the gate refuses
//!   the authorization and the extension rolls back;
//! - once the commit was authorized, an abort does not cancel the task: it waits for the task
//!   to end on its own and takes nothing, so a recorded outcome (or an unresolved one, which
//!   the task reports itself) is never reported as a cancelled turn, and a later turn started
//!   by the finishing task is never the abort's target.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::RunningTask;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GatePhase {
    #[default]
    Open,
    /// An abort won; no commit may be authorized.
    Aborted,
    /// A commit was authorized; aborts wait for the task to end.
    Authorized,
}

/// Orders one turn's terminal commit against aborts of its task.
#[derive(Default)]
struct TurnCommitGate(Mutex<GatePhase>);

impl TurnCommitGate {
    /// The turn's gate. Both sides create it on first use, atomically.
    fn of(turn: &TurnContext) -> Arc<Self> {
        turn.extension_data.get_or_init(Self::default)
    }

    fn phase(&self) -> std::sync::MutexGuard<'_, GatePhase> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Arbitrates an abort of `task`; call under the active-turn lock, before taking the task.
/// `Some` carries the task's end signal: its commit was authorized, so the caller must wait
/// for the task to end instead of aborting it, and must not take any task afterwards.
pub(crate) fn arbitrate_abort(task: &RunningTask) -> Option<watch::Receiver<bool>> {
    let gate = TurnCommitGate::of(&task.turn_context);
    let mut phase = gate.phase();
    match *phase {
        GatePhase::Open | GatePhase::Aborted => {
            *phase = GatePhase::Aborted;
            None
        }
        GatePhase::Authorized => Some(task.finished.clone()),
    }
}

/// Waits until a task whose commit won an abort race has ended.
pub(crate) async fn await_task_end(mut finished: watch::Receiver<bool>) {
    // A dropped sender also means the task ended.
    let _ = finished.wait_for(|finished| *finished).await;
}

/// Lets extensions record the terminal outcome of a regular task that ended normally with
/// `last_agent_message` and has no pending input.
pub(crate) async fn finalize(
    sess: &Session,
    turn: &TurnContext,
    last_agent_message: Option<&str>,
    cancellation_token: &CancellationToken,
) {
    let Some(message) = last_agent_message.filter(|message| !message.trim().is_empty()) else {
        return;
    };
    let gate = TurnCommitGate::of(turn);
    let authorize_commit = || {
        let mut phase = gate.phase();
        match *phase {
            GatePhase::Open if !cancellation_token.is_cancelled() => {
                *phase = GatePhase::Authorized;
                true
            }
            GatePhase::Authorized => true,
            GatePhase::Open | GatePhase::Aborted => false,
        }
    };
    for contributor in sess.services.extensions.turn_lifecycle_contributors() {
        let outcome = contributor
            .on_turn_finalize(TurnFinalizeInput {
                turn_id: &turn.sub_id,
                last_agent_message: message,
                authorize_commit: &authorize_commit,
                session_store: &sess.services.session_extension_data,
                thread_store: &sess.services.thread_extension_data,
                turn_store: turn.extension_data.as_ref(),
            })
            .await;
        match outcome {
            TurnFinalizeOutcome::NotHandled => continue,
            TurnFinalizeOutcome::Recorded => {}
            TurnFinalizeOutcome::Warning(message) => {
                sess.send_event(turn, EventMsg::Warning(WarningEvent { message }))
                    .await;
            }
        }
        break;
    }
}
