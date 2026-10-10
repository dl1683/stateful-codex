//! Host-ended Autonomous answers.
//!
//! A newly created Autonomous run's first answering task may end the run with its final
//! answer when the host observed that task do nothing but answer: no tool call of any kind,
//! no auxiliary command, no executable hook, no compaction, no failed response (Core records
//! these on the task's [`HostAnswerReservation`]). The model declares readiness with a
//! bounded outcome block at the end of that answer; only `disposition: answer` with
//! `open-issues: none` is admitted. The run then completes through the runtime's typed
//! host-answer transaction with an explicit basis: the answer is judged by the user, not
//! verified by the host. Every other task keeps the ordinary acceptance route.
//!
//! Candidacy is process-local and single-use: the app-server grants it only when it creates
//! a new Autonomous run on one idle thread, and the first turn that binds to the run consumes
//! it whatever that turn does. A restarted, foreign, previously active or continued run never
//! gets one.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_extension_api::HostAnswerPhase;
use codex_extension_api::HostAnswerReservation;
use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_stateful_runtime::HostAnswerCommit;
use codex_stateful_runtime::HostAnswerOutcome;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;

use crate::StatefulExtension;

/// The outcome block a pure answer ends with; see [`readiness`].
pub(crate) const OUTCOME_START: &str = "[stateful-outcome]";
const OUTCOME_END: &str = "[/stateful-outcome]";
const MAX_OUTCOME_BLOCK_BYTES: usize = 4 * 1024;
const MAX_OPEN_ISSUES: usize = 16;

const WORLD_STATE_ID: &str = "stateful_pure_answer";
const START_MARKER: &str = "<stateful_pure_answer>";
const END_MARKER: &str = "</stateful_pure_answer>";

/// Model guidance for the outcome block, shown only to a task that holds the candidacy.
const PURE_ANSWER_GUIDANCE: &str = "This Autonomous run may end with a pure answer. If the request is a question you can answer completely in this one message without any tool, answer it without calling any tool (not even stateful_run_update) and end the message with these four lines:
[stateful-outcome]
disposition: answer
open-issues: none
[/stateful-outcome]
The host then ends the run with your answer, marked as judged by the user rather than verified. Otherwise work normally; if you write the block without finishing, use disposition: continue, or disposition: blocked with each open issue on its own line starting with \"- \" after \"open-issues:\".";

/// Retracts the guidance in the turn after it was shown.
const PURE_ANSWER_ENDED: &str = "The pure-answer option of this run has ended: an outcome block no longer ends the run. Complete or block it with stateful_run_update as usual.";

/// Process-local, single-use host-answer candidacy per run.
#[derive(Clone, Default)]
pub struct HostAnswerCandidates(Arc<Mutex<CandidateState>>);

#[derive(Default)]
struct CandidateState {
    /// Granted runs and the one thread whose first turn may use the candidacy.
    granted: HashMap<String, String>,
    /// Runs this process granted or saw bound to a turn; none is ever granted again.
    decided: HashSet<String>,
}

impl HostAnswerCandidates {
    /// Grants the candidacy to a run this process just created on one idle thread. Returns
    /// false when this process already granted the run or already bound a turn to it.
    pub fn grant(&self, run_id: &str, thread_id: &str) -> bool {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.decided.insert(run_id.to_string()) {
            return false;
        }
        state
            .granted
            .insert(run_id.to_string(), thread_id.to_string());
        true
    }

    /// Consumes the run's candidacy for a turn that binds to it on `thread_id`. Returns
    /// whether that turn holds it. Any binding, with or without a grant, ends candidacy.
    pub(crate) fn take_for_turn(&self, run_id: &str, thread_id: &str) -> bool {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.decided.insert(run_id.to_string());
        state
            .granted
            .remove(run_id)
            .is_some_and(|granted_thread| granted_thread == thread_id)
    }
}

/// The readiness the model declared at the end of its final message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    /// `disposition: answer` with `open-issues: none`.
    Answered,
    /// Anything else: a missing or malformed block, `continue`, `blocked`, or open issues.
    NotAnswered(String),
}

/// Parses the outcome block that must end `message`.
pub(crate) fn readiness(message: &str) -> Readiness {
    let not_answered = |reason: &str| Readiness::NotAnswered(reason.to_string());
    let body = message.trim_end();
    let Some(start) = body.rfind(OUTCOME_START) else {
        return not_answered("the final message has no outcome block");
    };
    if body[..start].contains(OUTCOME_START) {
        return not_answered("the final message has more than one outcome block");
    }
    if start != 0 && !body[..start].ends_with('\n') {
        return not_answered("the outcome block does not start on its own line");
    }
    let block = &body[start..];
    if block.len() > MAX_OUTCOME_BLOCK_BYTES {
        return not_answered("the outcome block is too long");
    }
    let mut lines = block.lines().map(str::trim_end);
    let (Some(OUTCOME_START), Some(disposition)) = (lines.next(), lines.next()) else {
        return not_answered("the outcome block is malformed");
    };
    let Some(disposition) = disposition.strip_prefix("disposition: ") else {
        return not_answered("the outcome block has no disposition line");
    };
    let mut issues = Vec::new();
    let mut closed = false;
    match lines.next() {
        Some("open-issues: none") => {}
        Some("open-issues:") => {
            for line in lines.by_ref() {
                if line == OUTCOME_END {
                    closed = true;
                    break;
                }
                let Some(issue) = line
                    .strip_prefix("- ")
                    .filter(|issue| !issue.trim().is_empty())
                else {
                    return not_answered("an open issue line is malformed");
                };
                issues.push(issue);
                if issues.len() > MAX_OPEN_ISSUES {
                    return not_answered("the outcome block lists too many open issues");
                }
            }
            if issues.is_empty() {
                return not_answered("open-issues lists no issue; write open-issues: none");
            }
        }
        _ => return not_answered("the outcome block has no open-issues line"),
    }
    if !closed && lines.next() != Some(OUTCOME_END) {
        return not_answered("the outcome block is not closed");
    }
    if lines.next().is_some() {
        return not_answered("text follows the outcome block");
    }
    match (disposition, issues.is_empty()) {
        ("answer", true) => Readiness::Answered,
        ("answer", false) => not_answered("the answer declares open issues"),
        ("continue", _) => not_answered("the model declared that work continues"),
        ("blocked", _) => not_answered("the model declared that it is blocked"),
        _ => not_answered("the outcome block has an unknown disposition"),
    }
}

/// The guidance section for a task that holds an answering reservation, rendered once in
/// that turn and retracted once in the turn after. Returns the section and the bytes it adds
/// to the fresh window.
pub(crate) fn guidance_section(
    previous: Option<&serde_json::Value>,
    turn_id: &str,
    turn_store: &codex_extension_api::ExtensionData,
) -> (usize, codex_extension_api::WorldStateSectionContribution) {
    let field = |name: &str| previous.and_then(|previous| previous.get(name));
    let shown = turn_store
        .get::<HostAnswerReservation>()
        .is_some_and(|reservation| reservation.phase() == HostAnswerPhase::Answering);
    let same_turn = field("turnId").and_then(serde_json::Value::as_str) == Some(turn_id);
    let previously_shown = field("shown").and_then(serde_json::Value::as_bool) == Some(true);
    let body = match (same_turn, shown, previously_shown) {
        (true, _, _) | (false, false, false) => None,
        (false, true, _) => Some(PURE_ANSWER_GUIDANCE),
        (false, false, true) => Some(PURE_ANSWER_ENDED),
    };
    let bytes = body.map_or(0, |body| START_MARKER.len() + body.len() + END_MARKER.len());
    let turn_id = turn_id.to_string();
    let section = codex_extension_api::WorldStateSectionContribution::new(
        WORLD_STATE_ID,
        serde_json::json!({ "turnId": turn_id, "shown": shown }),
        move |previous| {
            let same_turn = match previous {
                codex_extension_api::PreviousWorldStateSection::Known(previous) => {
                    previous.get("turnId").and_then(serde_json::Value::as_str)
                        == Some(turn_id.as_str())
                }
                codex_extension_api::PreviousWorldStateSection::Absent
                | codex_extension_api::PreviousWorldStateSection::Unknown => false,
            };
            body.filter(|_| !same_turn).map(|body| {
                codex_extension_api::RenderedWorldStateFragment::new(
                    "developer",
                    (START_MARKER, END_MARKER),
                    body,
                )
            })
        },
    );
    (bytes, section)
}

/// Grants `turn_store` the run's candidacy when this turn is the first to bind to it.
pub(crate) fn reserve_for_turn(
    extension: &StatefulExtension,
    turn_store: &codex_extension_api::ExtensionData,
    run: &codex_stateful_runtime::StatefulRun,
    thread_id: &str,
    turn_id: &str,
) {
    let Some(autonomous) = extension.autonomous.as_ref() else {
        return;
    };
    let candidates = autonomous.admission.host_answer_candidates();
    if candidates.take_for_turn(run.id.as_str(), thread_id)
        && run.value.mode == WorkflowMode::Autonomous
        && run.status == StatefulRunStatus::Running
    {
        turn_store.insert(HostAnswerReservation::new(
            run.id.to_string(),
            thread_id.to_string(),
            turn_id.to_string(),
        ));
    }
}

/// Ends the reserved run with the finalizing task's answer, or declines.
pub(crate) async fn finalize(
    extension: &StatefulExtension,
    input: TurnFinalizeInput<'_>,
) -> TurnFinalizeOutcome {
    let reservation = input.reservation;
    let Some(services) = extension.services.as_ref() else {
        return TurnFinalizeOutcome::NotHandled;
    };
    if let Readiness::NotAnswered(reason) = readiness(input.last_agent_message) {
        return TurnFinalizeOutcome::Declined(reason);
    }
    let run_id = match StatefulRunId::parse(reservation.run_id()) {
        Ok(run_id) => run_id,
        Err(error) => return TurnFinalizeOutcome::Failed(error.to_string()),
    };
    let store = match services.runtime().await {
        Ok(store) => store,
        Err(error) => {
            return TurnFinalizeOutcome::Failed(format!(
                "The Stateful run store could not be opened, so the run was not ended with this answer: {error}"
            ));
        }
    };
    let run = match store.get_run(&run_id).await {
        Ok(Some(run)) => run,
        Ok(None) => return TurnFinalizeOutcome::Declined("the run no longer exists".to_string()),
        Err(error) => {
            return TurnFinalizeOutcome::Failed(format!(
                "The Stateful run could not be read, so it was not ended with this answer: {error}"
            ));
        }
    };
    let commit = HostAnswerCommit {
        run_id: run_id.clone(),
        expected_revision: run.revision,
        thread_id: reservation.thread_id().to_string(),
        turn_id: reservation.turn_id().to_string(),
        answer: input.last_agent_message.to_string(),
    };
    let authorize = || reservation.authorize_commit();
    match store
        .complete_run_with_host_answer(&commit, &authorize)
        .await
    {
        Ok(HostAnswerOutcome::Committed(run)) => {
            reservation.resolve_commit(/*committed*/ true);
            crate::autonomy::emit_run_updated(extension.event_sink.as_deref(), &run);
            TurnFinalizeOutcome::Committed
        }
        Ok(HostAnswerOutcome::Refused(reason)) => TurnFinalizeOutcome::Declined(reason),
        Ok(HostAnswerOutcome::NotAuthorized) => {
            TurnFinalizeOutcome::Declined("the turn was aborted before the commit".to_string())
        }
        Err(error) => {
            resolve_uncertain_commit(extension, &store, reservation, &run_id, error).await
        }
    }
}

/// A storage error after commit authorization leaves the outcome unknown: re-read the
/// durable record before reporting either way.
async fn resolve_uncertain_commit(
    extension: &StatefulExtension,
    store: &codex_stateful_runtime::StatefulRunStore,
    reservation: &HostAnswerReservation,
    run_id: &StatefulRunId,
    error: codex_stateful_runtime::StatefulRunStoreError,
) -> TurnFinalizeOutcome {
    if reservation.phase() != HostAnswerPhase::Committing {
        return TurnFinalizeOutcome::Failed(format!(
            "The Stateful run was not ended with this answer: {error}"
        ));
    }
    let committed = match store.host_answer(run_id).await {
        Ok(Some(record)) => record.turn_id == reservation.turn_id(),
        Ok(None) => false,
        Err(reread) => {
            // Unknown either way: report no success, and let a later read show the truth.
            reservation.resolve_commit(/*committed*/ false);
            return TurnFinalizeOutcome::Failed(format!(
                "Ending the Stateful run with this answer may not have been saved ({error}); its state could not be re-read ({reread}). Read the run before relying on it."
            ));
        }
    };
    reservation.resolve_commit(committed);
    if committed {
        if let Ok(Some(run)) = store.get_run(run_id).await {
            crate::autonomy::emit_run_updated(extension.event_sink.as_deref(), &run);
        }
        return TurnFinalizeOutcome::Committed;
    }
    TurnFinalizeOutcome::Failed(format!(
        "The Stateful run was not ended with this answer: {error}"
    ))
}

#[cfg(test)]
#[path = "host_answer_tests.rs"]
mod tests;
