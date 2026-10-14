//! "Answered, unverified": an Autonomous run that ends with the agent's final answer.
//!
//! A genuine substantive final of a running Autonomous run ends Answered or Partial through
//! the host's commit gate. Neither outcome certifies work or satisfies acceptance criteria.
//! The shared classification keeps status-only replies, explicit continuation and this
//! turn's model-recorded next work on their ordinary route; a valid answer trailer overrides
//! the progress guard. The runtime collects open and not-certified items transactionally.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use codex_stateful_runtime::AnsweredRunCommit;
use codex_stateful_runtime::AnsweredRunOutcome;
use codex_stateful_runtime::HostAnswerOutcome;
use codex_stateful_runtime::HostAnswerRecord;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::partial_result_items;

use crate::StatefulEventSink;
use crate::final_reply::FinalReply;
use crate::final_reply::ModelProgress;

/// Cap of the partial-outcome disclosure (the status-only diagnostic's cap).
const MAX_DISCLOSURE_BYTES: usize = 1_200;
const PARTIAL_DISCLOSURE: &str = "Stateful: this run ended partial; it is not certified by this terminal answer path. Open or not certified: ";
const UNREAD_ENDING: &str = "The Stateful run ended with this answer; how it ended could not be re-read. Read the run before relying on it.";

/// Model guidance for the outcome block, part of the Autonomous mode obligation.
pub(crate) const ANSWER_OUTCOME_GUIDANCE: &str = "Give a substantive final answer; an eligible final ends Answered or Partial. Neither certifies work. While work remains, work or record next with obligation_update. A trailing [stateful-outcome] block uses separate lines: disposition: continue or answer; open-issues: none or unfinished items; [/stateful-outcome]. After recorded next work, answer explicitly chooses the answer. Retained blockers require Partial; resolve only with obligation_update resolvedIssues. Settle pending steering; size, binding and authorization gates apply.";

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

/// Whether the finalized message is the turn's last assistant message and not commentary.
pub(crate) fn genuine_final(input: &TurnFinalizeInput<'_>) -> bool {
    input
        .turn_store
        .get::<LastAgentMessage>()
        .is_some_and(|last| !last.commentary && last.text == input.last_agent_message)
}

/// Ends the turn's run as answered, unverified, when the turn's final message is a genuine,
/// ready answer; otherwise records nothing and the run keeps its ordinary route.
pub(crate) async fn finalize(
    store: &StatefulRunStore,
    event_sink: Option<&dyn StatefulEventSink>,
    run_id: StatefulRunId,
    thread_id: &str,
    input: &TurnFinalizeInput<'_>,
    progress: ModelProgress,
) -> TurnFinalizeOutcome {
    if !genuine_final(input) {
        return TurnFinalizeOutcome::NotHandled;
    }
    let (result, issues) = match crate::final_reply::classify(input.last_agent_message, progress) {
        FinalReply::Deliver { body, issues } => (body, issues),
        FinalReply::KeepRunning(reason) => {
            tracing::debug!(turn_id = input.turn_id, reason, "the run keeps its route");
            return TurnFinalizeOutcome::NotHandled;
        }
        FinalReply::StatusOnly(_) => return TurnFinalizeOutcome::NotHandled,
    };
    let commit = AnsweredRunCommit {
        run_id,
        thread_id: thread_id.to_string(),
        turn_id: input.turn_id.to_string(),
        answer: input.last_agent_message.to_string(),
        result: result.to_string(),
        trailer_issues: issues,
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
        Ok(AnsweredRunOutcome::Answered(run, record)) => {
            crate::autonomy::emit_run_updated(event_sink, &run);
            return outcome_disclosure(&run, &record);
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
        Ok(record) => match store.get_run(&commit.run_id).await {
            Ok(Some(run)) => {
                crate::autonomy::emit_run_updated(event_sink, &run);
                outcome_disclosure(&run, &record)
            }
            Ok(None) | Err(_) => TurnFinalizeOutcome::Warning(UNREAD_ENDING.to_string()),
        },
        Err(message) => TurnFinalizeOutcome::Warning(message),
    }
}

/// What a re-read says about a commit whose result was `error`: this turn's durable answer
/// record, otherwise the truthful message (not saved, or unknown).
fn after_unknown_commit(
    turn_id: &str,
    error: &StatefulRunStoreError,
    reread: Result<Option<HostAnswerRecord>, StatefulRunStoreError>,
) -> Result<HostAnswerRecord, String> {
    match reread {
        Ok(Some(record)) if record.turn_id == turn_id => Ok(record),
        Ok(_) => Err(format!(
            "Ending the Stateful run with this answer was not saved ({error}), so it continues as usual."
        )),
        Err(reread) => Err(format!(
            "Ending the Stateful run with this answer may or may not have been saved ({error}), and its state could not be re-read ({reread}). Read the run before relying on how it ended."
        )),
    }
}

/// The finalization outcome of a durable answered ending: `Recorded` for an answered run, a
/// bounded warning listing the open or not-certified items for a partial one. The items are
/// read from the run's durable result, so every success path discloses the same text.
pub(crate) fn outcome_disclosure(
    run: &StatefulRun,
    record: &HostAnswerRecord,
) -> TurnFinalizeOutcome {
    match record.outcome() {
        HostAnswerOutcome::Answered => TurnFinalizeOutcome::Recorded,
        HostAnswerOutcome::Partial => TurnFinalizeOutcome::Warning(partial_disclosure(
            run.result.as_deref().unwrap_or_default(),
        )),
    }
}

/// The disclosure of a partial result, at most [`MAX_DISCLOSURE_BYTES`].
fn partial_disclosure(result: &str) -> String {
    let parsed = partial_result_items(result).unwrap_or_default();
    let listed = parsed.items.join("; ");
    let more =
        (parsed.omitted > 0).then(|| format!(" [+{} more; see the run result]", parsed.omitted));
    let text = format!(
        "{PARTIAL_DISCLOSURE}{listed}{}",
        more.as_deref().unwrap_or_default()
    );
    if text.len() <= MAX_DISCLOSURE_BYTES {
        return text;
    }
    let suffix = more.unwrap_or_else(|| " [shortened; see the run result]".to_string());
    let excerpt = crate::deliverable::utf8_prefix(
        &listed,
        MAX_DISCLOSURE_BYTES - PARTIAL_DISCLOSURE.len() - suffix.len(),
    );
    format!("{PARTIAL_DISCLOSURE}{excerpt}{suffix}")
}

#[cfg(test)]
#[path = "answered_tests.rs"]
mod tests;
