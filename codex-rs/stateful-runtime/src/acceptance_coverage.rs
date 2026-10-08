//! Request coverage: which goal sentences no binding criterion covers yet.
//!
//! Coverage is derived from durable run state on every evaluation, so a bounded proposal
//! batch, a full ledger or a restart can never make a sentence disappear. A goal sentence is
//! covered only when every non-whitespace byte of it lies in the span of an active required
//! user-bound criterion, a pending omission proposal (which gates on its own), or a dismissed
//! proposal that carries a host-validated receipt. Derived, optional and retired criteria
//! never cover user text. Partial coverage proposes the whole sentence.

use crate::AcceptanceLedger;
use crate::AcceptanceOrigin;
use crate::AcceptanceState;
use crate::RequestSpan;

/// Proposals one omission pass may add.
pub const MAX_OMISSION_PROPOSALS: usize = 8;

/// Whether the run may complete without request coverage: a one-sentence request with no
/// criteria, no observed command execution and no observed workspace mutation. Anything else
/// owes coverage of every goal sentence.
pub fn coverage_exempt(goal: &str, ledger: &AcceptanceLedger) -> bool {
    sentences(goal).len() <= 1
        && ledger.criteria.is_empty()
        && ledger.observed_executions == 0
        && ledger.workspace_generation == 0
}

/// Goal sentences not fully covered by binding criteria or proposals, in goal order.
pub fn uncovered_sentences(goal: &str, ledger: &AcceptanceLedger) -> Vec<RequestSpan> {
    if coverage_exempt(goal, ledger) {
        return Vec::new();
    }
    let binding = ledger
        .criteria
        .iter()
        .filter(|criterion| {
            criterion.origin != AcceptanceOrigin::Derived
                && match criterion.state {
                    AcceptanceState::Active => criterion.required,
                    AcceptanceState::Proposed => true,
                    AcceptanceState::Dismissed => criterion.dismissal.is_some(),
                    AcceptanceState::Retired => false,
                }
        })
        .filter_map(|criterion| criterion.request_span)
        .collect::<Vec<_>>();
    let bytes = goal.as_bytes();
    sentences(goal)
        .into_iter()
        .filter(|sentence| {
            (sentence.start..sentence.end).any(|index| {
                !bytes[index].is_ascii_whitespace()
                    && !binding
                        .iter()
                        .any(|span| span.start <= index && index < span.end)
            })
        })
        .collect()
}

/// Non-empty goal sentences, split at line breaks, semicolons and sentence ends (a period,
/// question or exclamation mark followed by whitespace or the end), trimmed of list markers.
pub fn sentences(goal: &str) -> Vec<RequestSpan> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = goal.as_bytes();
    for (index, character) in goal.char_indices() {
        // (end of this segment, start of the next): separators are dropped, sentence
        // punctuation stays with its sentence.
        let boundary = match character {
            '\n' | ';' => Some((index, index + 1)),
            '.' | '!' | '?' => bytes
                .get(index + 1)
                .is_none_or(u8::is_ascii_whitespace)
                .then_some((index + 1, index + 1)),
            _ => None,
        };
        if let Some((end, next)) = boundary {
            push_segment(goal, start, end, &mut segments);
            start = next;
        }
    }
    push_segment(goal, start, goal.len(), &mut segments);
    segments
}

fn push_segment(goal: &str, start: usize, end: usize, segments: &mut Vec<RequestSpan>) {
    let Some(raw) = goal.get(start..end) else {
        return;
    };
    let trimmed = raw.trim().trim_start_matches(['-', '*', '•']).trim();
    if !trimmed.chars().any(char::is_alphanumeric) {
        return;
    }
    let offset = start + raw.find(trimmed).unwrap_or_default();
    segments.push(RequestSpan {
        start: offset,
        end: offset + trimmed.len(),
    });
}

#[cfg(test)]
#[path = "acceptance_coverage_tests.rs"]
mod tests;
