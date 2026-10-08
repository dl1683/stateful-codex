//! Host-side acceptance policy: the independent omission check over the user's request and
//! the risk-proportional effort policy.
//!
//! Both read structure, not words. The omission check segments the stored goal into
//! sentences and proposes, for review, every sentence no criterion covers; it never guesses
//! which sentences matter, and it can add proposals but never edits or removes a criterion, so
//! it cannot weaken a user constraint. Risk is read from the ledger's declared structure
//! (deliverables, artifacts, required criteria, irreversible milestones). Effort is
//! proportional to risk, not a minimum runtime: a one-sentence lookup that changed nothing gets
//! no omission check and no reserve, and a correct short run can finish immediately.

use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::RequestSpan;
use codex_stateful_runtime::RunBudget;

/// Proposals one omission check may add.
pub(crate) const MAX_OMISSION_PROPOSALS: usize = 8;
/// Longest proposal statement; longer sentences are cut at a character boundary.
const MAX_PROPOSAL_BYTES: usize = 480;

/// Why a task needs reserved verification effort, read from the declared ledger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RiskProfile {
    /// Two or more declared deliverables or artifacts.
    pub(crate) multi_artifact: bool,
    /// Two or more required criteria must all hold.
    pub(crate) all_or_nothing: bool,
    /// A criterion guards an irreversible step.
    pub(crate) irreversible: bool,
}

impl RiskProfile {
    pub(crate) fn assess(ledger: &AcceptanceLedger) -> Self {
        let active = ledger
            .criteria
            .iter()
            .filter(|criterion| criterion.state == AcceptanceState::Active)
            .collect::<Vec<_>>();
        let mut artifacts = active
            .iter()
            .flat_map(|criterion| criterion.artifacts.iter())
            .collect::<Vec<_>>();
        artifacts.sort_unstable();
        artifacts.dedup();
        let deliverables = active
            .iter()
            .filter(|criterion| criterion.kind == AcceptanceKind::Deliverable)
            .count();
        Self {
            multi_artifact: artifacts.len() >= 2 || deliverables >= 2,
            all_or_nothing: active.iter().filter(|criterion| criterion.required).count() >= 2,
            irreversible: active.iter().any(|criterion| criterion.milestone.is_some()),
        }
    }

    pub(crate) fn any(self) -> bool {
        self.multi_artifact || self.all_or_nothing || self.irreversible
    }

    pub(crate) fn labels(self) -> Vec<&'static str> {
        [
            (self.multi_artifact, "multi-artifact"),
            (self.all_or_nothing, "all-or-nothing"),
            (self.irreversible, "irreversible milestone"),
        ]
        .into_iter()
        .filter_map(|(present, label)| present.then_some(label))
        .collect()
    }
}

/// A task is substantial, and owes the omission check before completion, once it changed the
/// workspace, declared criteria, or its request has more than one sentence. A one-sentence
/// lookup that changed nothing completes without any acceptance ritual.
pub(crate) fn is_substantial(goal: &str, ledger: &AcceptanceLedger) -> bool {
    ledger.workspace_generation > 0 || !ledger.criteria.is_empty() || sentences(goal).len() >= 2
}

/// Goal sentences that no criterion (in any state) overlaps, as reviewable proposals, plus the
/// number of further uncovered sentences beyond the cap.
pub(crate) fn omission_proposals(
    goal: &str,
    ledger: &AcceptanceLedger,
) -> (Vec<(String, RequestSpan)>, usize) {
    let covered = |span: &RequestSpan| {
        ledger.criteria.iter().any(|criterion| {
            criterion
                .request_span
                .is_some_and(|existing| existing.start < span.end && span.start < existing.end)
        })
    };
    let uncovered = sentences(goal)
        .into_iter()
        .filter(|span| !covered(span))
        .collect::<Vec<_>>();
    let omitted = uncovered.len().saturating_sub(MAX_OMISSION_PROPOSALS);
    let proposals = uncovered
        .into_iter()
        .take(MAX_OMISSION_PROPOSALS)
        .filter_map(|span| {
            let quote = span.quote(goal)?;
            let statement = if quote.len() <= MAX_PROPOSAL_BYTES {
                quote.to_string()
            } else {
                let mut end = MAX_PROPOSAL_BYTES;
                while !quote.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", quote[..end].trim_end())
            };
            Some((single_line(&statement), span))
        })
        .collect();
    (proposals, omitted)
}

/// Non-empty goal sentences, split at line breaks, semicolons and sentence ends (a period,
/// question or exclamation mark followed by whitespace or the end), trimmed of list markers.
fn sentences(goal: &str) -> Vec<RequestSpan> {
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

/// Reserved verification and rework allowance: none for low-risk work, otherwise a fifth of
/// the continuation and elapsed budgets (at least two continuations, at most half).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct VerificationReserve {
    pub(crate) continuations: u32,
    pub(crate) seconds: u32,
}

impl VerificationReserve {
    pub(crate) fn for_risk(risk: RiskProfile, budget: RunBudget) -> Self {
        if !risk.any() {
            return Self::default();
        }
        let half = budget.max_continuations / 2;
        Self {
            continuations: (budget.max_continuations / 5).max(2).min(half),
            seconds: budget.max_elapsed_seconds / 5,
        }
    }
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "acceptance_policy_tests.rs"]
mod tests;
