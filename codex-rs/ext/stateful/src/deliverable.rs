//! The final message must carry the deliverable: a genuine final reply bound to the turn's
//! run that is only a status line gets a bounded, non-fatal warning that never retries,
//! rejects, blocks or changes what was committed. It quotes this turn's own recorded result
//! only while the run keeps that commit's revision and status.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;

use crate::answered::Readiness;
use crate::run_world_state::status_name;

/// Caps: the warning, its quoted status line, an existing warning; receipts kept (oldest out).
const MAX_WARNING_BYTES: usize = 1_200;
const MAX_REPLY_BYTES: usize = 60;
const MAX_EXISTING_WARNING_BYTES: usize = 400;
const MAX_TURN_RECEIPTS: usize = 64;
const STATUS_ONLY: &str =
    "This final reply may omit the deliverable: it contains only a status line.";

/// The run a successful commit in a turn returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunCommitReceipt {
    run_id: StatefulRunId,
    revision: u64,
    status: StatefulRunStatus,
    result: Option<String>,
}

/// Process-local receipts of the run commits each turn made, keyed by turn id.
#[derive(Clone, Default)]
pub(crate) struct TurnRunCommits(Arc<Mutex<VecDeque<(String, RunCommitReceipt)>>>);

impl TurnRunCommits {
    /// Records the run a successful commit returned; a later commit in the turn replaces it.
    pub(crate) fn record(&self, turn_id: &str, run: &StatefulRun) {
        let mut receipts = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        receipts.retain(|(turn, _)| turn != turn_id);
        receipts.push_back((
            turn_id.to_string(),
            RunCommitReceipt {
                run_id: run.id.clone(),
                revision: run.revision,
                status: run.status,
                result: run.result.clone(),
            },
        ));
        while receipts.len() > MAX_TURN_RECEIPTS {
            receipts.pop_front();
        }
    }

    pub(crate) fn take(&self, turn_id: &str) -> Option<RunCommitReceipt> {
        let mut receipts = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let position = receipts.iter().position(|(turn, _)| turn == turn_id)?;
        receipts.remove(position).map(|(_, receipt)| receipt)
    }
}

/// Whether `text` is only status lines such as "Task completed." or "**Done!** All done.":
/// every nonempty sentence or line must be a status phrase.
pub(crate) fn is_status_only(text: &str) -> bool {
    let mut phrases = strip_wrapper(text.trim())
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|phrase| !phrase.is_empty())
        .peekable();
    phrases.peek().is_some() && phrases.all(is_status_phrase)
}

fn strip_wrapper(text: &str) -> &str {
    ["**", "*", "_", "`"]
        .iter()
        .find_map(|wrapper| text.strip_prefix(wrapper)?.strip_suffix(wrapper))
        .unwrap_or(text)
}

fn is_status_phrase(text: &str) -> bool {
    let text = strip_wrapper(text);
    let words = text
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let line = words.trim_end_matches(['.', '!']).trim_end();
    let line = line.strip_suffix(" successfully").unwrap_or(line);
    let state = |state: &str| matches!(state, "complete" | "completed" | "finished" | "done");
    matches!(line, "done" | "all done" | "finished" | "completed")
        || ["run ", "task ", "work "]
            .iter()
            .any(|prefix| line.strip_prefix(prefix).is_some_and(state))
}

/// The status line of this turn's genuine final reply, when that reply is status-only: an
/// accepted answer block is classified by its body, any other message whole.
pub(crate) fn status_only_reply<'a>(input: &TurnFinalizeInput<'a>) -> Option<&'a str> {
    if !crate::answered::genuine_final(input) {
        return None;
    }
    let text = match crate::answered::readiness(input.last_agent_message) {
        Readiness::Answered(body) => body,
        Readiness::NotAnswered(_) => input.last_agent_message.trim(),
    };
    is_status_only(text).then_some(text)
}

/// At finalize: no receipt for the bound run, the run still at the receipt's revision and
/// status, or the run changed since (or could not be read).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReceiptCheck {
    NoReceipt,
    Matched(RunCommitReceipt),
    Changed(RunCommitReceipt),
}

impl ReceiptCheck {
    pub(crate) async fn read(
        store: &StatefulRunStore,
        receipt: Option<RunCommitReceipt>,
        bound_run: &StatefulRunId,
    ) -> Self {
        let Some(receipt) = receipt.filter(|receipt| receipt.run_id == *bound_run) else {
            return Self::NoReceipt;
        };
        match store.get_run(&receipt.run_id).await {
            Ok(Some(run)) if run.revision == receipt.revision && run.status == receipt.status => {
                Self::Matched(receipt)
            }
            Ok(_) | Err(_) => Self::Changed(receipt),
        }
    }
}

/// Adds the status-only diagnostic to the turn's finalization outcome without changing what
/// was committed: core treats `Recorded` and `Warning` alike apart from the warning event.
pub(crate) fn warn(
    outcome: TurnFinalizeOutcome,
    reply: &str,
    check: &ReceiptCheck,
) -> TurnFinalizeOutcome {
    match outcome {
        TurnFinalizeOutcome::NotHandled | TurnFinalizeOutcome::Recorded => {
            TurnFinalizeOutcome::Warning(diagnostic(reply, check, MAX_WARNING_BYTES))
        }
        TurnFinalizeOutcome::Warning(existing) => {
            let existing = utf8_prefix(&existing, MAX_EXISTING_WARNING_BYTES);
            let diagnostic = diagnostic(reply, check, MAX_WARNING_BYTES - existing.len() - 1);
            TurnFinalizeOutcome::Warning(format!("{existing}\n{diagnostic}"))
        }
    }
}

/// The diagnostic text, at most `budget` bytes.
fn diagnostic(reply: &str, check: &ReceiptCheck, budget: usize) -> String {
    let receipt = match check {
        ReceiptCheck::NoReceipt => return STATUS_ONLY.to_string(),
        ReceiptCheck::Changed(receipt) => {
            return format!(
                "This final reply may omit the deliverable: it contains only a status line. This turn recorded the run as {} at revision {}; it has changed since, so its result is not quoted.",
                status_name(receipt.status),
                receipt.revision
            );
        }
        ReceiptCheck::Matched(receipt) => receipt,
    };
    let prefix = format!(
        "This final reply may omit the deliverable: it contains only a status line (\"{}\"). This turn's recorded result ({}, revision {}): ",
        utf8_prefix(reply, MAX_REPLY_BYTES),
        status_name(receipt.status),
        receipt.revision
    );
    let result = receipt.result.as_deref().unwrap_or_default();
    if prefix.len() + result.len() <= budget {
        return format!("{prefix}{result}");
    }
    let total = result.len();
    // Reserved at the longest a marker can be: the cut never exceeds the total.
    let reserved = format!(" [shortened at {total} of {total} bytes]").len();
    let excerpt = utf8_prefix(result, budget.saturating_sub(prefix.len() + reserved));
    format!(
        "{prefix}{excerpt} [shortened at {} of {total} bytes]",
        excerpt.len()
    )
}

/// The longest prefix of `text` within `max_bytes` that ends on a character boundary.
fn utf8_prefix(text: &str, max_bytes: usize) -> &str {
    &text[..text.floor_char_boundary(max_bytes)]
}

#[cfg(test)]
#[path = "deliverable_tests.rs"]
mod tests;
