//! "Answered, unverified": an Autonomous run that ends with the agent's final answer.
//!
//! When a turn bound to a running Autonomous run ends with a final answer whose last lines are
//! the outcome block below, the run ends as Answered: a truthful label for "the agent
//! answered; the host verified nothing". It is neither Completed nor Blocked, satisfies no
//! acceptance criterion, and asserts nothing about what the turn did, so it needs no
//! observation of tools, hooks or other work. A run that did work and then answers is
//! labelled the same way: its work stays unverified.
//!
//! Only a genuine final answer qualifies: the block must end the turn's final assistant
//! message, that message must not be commentary, and real answer text must precede the block.
//! The run's terminal write happens inside the answering task through the host's commit
//! gate; see the runtime's `end_run_answered`.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use codex_stateful_runtime::AnsweredRunCommit;
use codex_stateful_runtime::AnsweredRunOutcome;
use codex_stateful_runtime::HostAnswerRecord;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;

use crate::StatefulEventSink;

const OUTCOME_START: &str = "[stateful-outcome]";
const OUTCOME_END: &str = "[/stateful-outcome]";
const MAX_OUTCOME_BLOCK_BYTES: usize = 4 * 1024;
const MAX_OPEN_ISSUES: usize = 16;

/// Model guidance for the outcome block, part of the Autonomous mode obligation.
pub(crate) const ANSWER_OUTCOME_GUIDANCE: &str = "A complete answer needing no further work may instead end the final message with four lines: [stateful-outcome], disposition: answer, open-issues: none, [/stateful-outcome]; the run then ends answered, not verified.";

/// The last assistant message the turn completed, as the host emitted it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LastAgentMessage {
    text: String,
    commentary: bool,
}

impl LastAgentMessage {
    pub(crate) fn from_item(item: &AgentMessageItem) -> Self {
        Self {
            text: item
                .content
                .iter()
                .map(|entry| match entry {
                    AgentMessageContent::Text { text } => text.as_str(),
                })
                .collect(),
            commentary: matches!(item.phase, Some(MessagePhase::Commentary)),
        }
    }
}

/// The readiness the model declared at the end of its final message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Readiness<'a> {
    /// `disposition: answer` with `open-issues: none`, after this answer text.
    Answered(&'a str),
    /// Anything else: a missing or malformed block, `continue`, `blocked`, open issues, text
    /// after the block, or no answer before it.
    NotAnswered(&'static str),
}

/// Parses the outcome block that must end `message`.
pub(crate) fn readiness(message: &str) -> Readiness<'_> {
    use Readiness::NotAnswered;
    let body = message.trim_end();
    let Some(start) = body.rfind(OUTCOME_START) else {
        return NotAnswered("the final message has no outcome block");
    };
    let answer = &body[..start];
    if answer.contains(OUTCOME_START) {
        return NotAnswered("the final message has more than one outcome block");
    }
    if answer.trim().is_empty() {
        return NotAnswered("no answer precedes the outcome block");
    }
    if !answer.ends_with('\n') {
        return NotAnswered("the outcome block does not start on its own line");
    }
    let block = &body[start..];
    if block.len() > MAX_OUTCOME_BLOCK_BYTES {
        return NotAnswered("the outcome block is too long");
    }
    let mut lines = block.lines().map(str::trim_end);
    let (Some(OUTCOME_START), Some(disposition)) = (lines.next(), lines.next()) else {
        return NotAnswered("the outcome block is malformed");
    };
    let Some(disposition) = disposition.strip_prefix("disposition: ") else {
        return NotAnswered("the outcome block has no disposition line");
    };
    let mut issues = 0;
    let mut closed = false;
    match lines.next() {
        Some("open-issues: none") => {}
        Some("open-issues:") => {
            for line in lines.by_ref() {
                if line == OUTCOME_END {
                    closed = true;
                    break;
                }
                if line
                    .strip_prefix("- ")
                    .is_none_or(|issue| issue.trim().is_empty())
                {
                    return NotAnswered("an open issue line is malformed");
                }
                issues += 1;
                if issues > MAX_OPEN_ISSUES {
                    return NotAnswered("the outcome block lists too many open issues");
                }
            }
            if issues == 0 {
                return NotAnswered("open-issues lists no issue");
            }
        }
        _ => return NotAnswered("the outcome block has no open-issues line"),
    }
    if !closed && lines.next() != Some(OUTCOME_END) {
        return NotAnswered("the outcome block is not closed");
    }
    if lines.next().is_some() {
        return NotAnswered("text follows the outcome block");
    }
    match (disposition, issues) {
        ("answer", 0) => Readiness::Answered(answer.trim()),
        ("answer", _) => NotAnswered("the answer declares open issues"),
        ("continue", _) => NotAnswered("the model declared that work continues"),
        ("blocked", _) => NotAnswered("the model declared that it is blocked"),
        _ => NotAnswered("the outcome block has an unknown disposition"),
    }
}

/// Ends the turn's run as answered, unverified, when the turn's final message is a genuine,
/// ready answer; otherwise records nothing and the run keeps its ordinary route.
pub(crate) async fn finalize(
    store: &StatefulRunStore,
    event_sink: Option<&dyn StatefulEventSink>,
    run_id: StatefulRunId,
    thread_id: &str,
    input: &TurnFinalizeInput<'_>,
) -> TurnFinalizeOutcome {
    let genuine_final = input
        .turn_store
        .get::<LastAgentMessage>()
        .is_some_and(|last| !last.commentary && last.text == input.last_agent_message);
    if !genuine_final {
        return TurnFinalizeOutcome::NotHandled;
    }
    let result = match readiness(input.last_agent_message) {
        Readiness::Answered(result) => result,
        Readiness::NotAnswered(reason) => {
            tracing::debug!(turn_id = input.turn_id, reason, "the run keeps its route");
            return TurnFinalizeOutcome::NotHandled;
        }
    };
    let commit = AnsweredRunCommit {
        run_id,
        thread_id: thread_id.to_string(),
        turn_id: input.turn_id.to_string(),
        answer: input.last_agent_message.to_string(),
        result: result.to_string(),
    };
    let authorized = AtomicBool::new(false);
    let authorize = || {
        let granted = (input.authorize_commit)();
        authorized.store(granted, Ordering::SeqCst);
        granted
    };
    let outcome = store.end_run_answered(&commit, &authorize).await;
    resolve(
        store,
        event_sink,
        &commit,
        authorized.load(Ordering::SeqCst),
        outcome,
    )
    .await
}

/// Turns the transaction's result into the turn's finalization outcome. A storage error after
/// authorization leaves the commit unknown: the durable record is re-read before reporting,
/// and an unknown outcome is reported as such, never as a cancellation.
pub(crate) async fn resolve(
    store: &StatefulRunStore,
    event_sink: Option<&dyn StatefulEventSink>,
    commit: &AnsweredRunCommit,
    authorized: bool,
    outcome: Result<AnsweredRunOutcome, StatefulRunStoreError>,
) -> TurnFinalizeOutcome {
    let error = match outcome {
        Ok(AnsweredRunOutcome::Answered(run)) => {
            crate::autonomy::emit_run_updated(event_sink, &run);
            return TurnFinalizeOutcome::Recorded;
        }
        Ok(AnsweredRunOutcome::NotEligible | AnsweredRunOutcome::NotAuthorized) => {
            return TurnFinalizeOutcome::NotHandled;
        }
        Ok(AnsweredRunOutcome::Refused(reason)) => {
            return TurnFinalizeOutcome::Warning(format!(
                "The Stateful run did not end with this answer: {reason}. It continues as usual."
            ));
        }
        Err(error) => error,
    };
    if !authorized {
        return TurnFinalizeOutcome::Warning(format!(
            "The Stateful run could not be ended with this answer, so it continues as usual: {error}"
        ));
    }
    match after_unknown_commit(
        &commit.turn_id,
        &error,
        store.host_answer(&commit.run_id).await,
    ) {
        Ok(()) => {
            if let Ok(Some(run)) = store.get_run(&commit.run_id).await {
                crate::autonomy::emit_run_updated(event_sink, &run);
            }
            TurnFinalizeOutcome::Recorded
        }
        Err(message) => TurnFinalizeOutcome::Warning(message),
    }
}

/// What a re-read says about a commit whose result was `error`: `Ok` when this turn's answer
/// is durable, otherwise the truthful message (not saved, or unknown).
fn after_unknown_commit(
    turn_id: &str,
    error: &StatefulRunStoreError,
    reread: Result<Option<HostAnswerRecord>, StatefulRunStoreError>,
) -> Result<(), String> {
    match reread {
        Ok(Some(record)) if record.turn_id == turn_id => Ok(()),
        Ok(_) => Err(format!(
            "Ending the Stateful run with this answer was not saved ({error}), so it continues as usual."
        )),
        Err(reread) => Err(format!(
            "Ending the Stateful run with this answer may or may not have been saved ({error}), and its state could not be re-read ({reread}). Read the run before relying on how it ended."
        )),
    }
}

#[cfg(test)]
#[path = "answered_tests.rs"]
mod tests;
