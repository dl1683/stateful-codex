//! Write economy for model-recorded knowledge.
//!
//! The host already keeps every request and final answer, so a model record is worth its
//! cost only when it adds a durable outcome a later session would otherwise lose. Two
//! cheap, deterministic checks run before a record is stored:
//!
//! - an exact copy of an active agent entry of the same kind is a no-op: no new entry, no
//!   new revision and no saved receipt;
//! - a recognisable session-progress or worktree-status summary is refused, because it
//!   restates what the conversation record and Git already hold.
//!
//! These checks do not claim to remove all semantic redundancy: a reworded duplicate is
//! still stored. New decisions, reasons, rejected approaches, failures and open questions
//! are never refused here.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardKind;

/// What recording one item did.
#[derive(Debug)]
pub(crate) enum RecordOutcome {
    /// A new entry (or the idempotent replay of one) was committed.
    Created(BlackboardEntry),
    /// The same wording is already current knowledge; nothing was written.
    AlreadyPresent(BlackboardEntry),
}

/// Openings that mark a record as a narrative of the session or the checkout rather than
/// a durable outcome. Matched case-insensitively at the start of the trimmed content.
const ROUTINE_SUMMARY_OPENINGS: &[&str] = &[
    "yesterday's ",
    "yesterday we ",
    "yesterday, we ",
    "today we ",
    "today, we ",
    "earlier today",
    "in this session",
    "this session ",
    "this session,",
    "last session",
    "previous session",
    "session summary",
    "summary of this session",
    "summary of today",
    "progress update",
    "progress so far",
    "progress:",
    "status update",
    "status:",
    "current status",
    "work so far",
    "worktree ",
    "worktree:",
    "working tree ",
    "working tree:",
    "uncommitted changes",
    "git status",
];

/// Why `content` is a routine session or worktree summary that should not become project
/// knowledge, or `None`. Only descriptive kinds are checked: a decision, rejected
/// approach, failure or question is always a candidate outcome.
pub(crate) fn routine_summary_refusal(kind: BlackboardKind, content: &str) -> Option<String> {
    match kind {
        BlackboardKind::Fact | BlackboardKind::Claim | BlackboardKind::Note => {}
        BlackboardKind::Instruction
        | BlackboardKind::Number
        | BlackboardKind::Decision
        | BlackboardKind::Strategy
        | BlackboardKind::Question
        | BlackboardKind::Contradiction
        | BlackboardKind::Failure
        | BlackboardKind::RejectedApproach
        | BlackboardKind::Signal => return None,
    }
    let opening = content.trim_start().to_lowercase();
    ROUTINE_SUMMARY_OPENINGS
        .iter()
        .any(|prefix| opening.starts_with(prefix))
        .then(|| {
            "routineSummary: the host already keeps every request and final answer, and Git keeps the worktree; a session-progress or status summary is not stored. Record only a new decision with its reason, a verified recipe, or a fact a later session would lose".to_string()
        })
}

#[cfg(test)]
#[path = "capture_policy_tests.rs"]
mod tests;
