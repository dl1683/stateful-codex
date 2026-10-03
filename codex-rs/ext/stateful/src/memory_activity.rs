//! What project memory holds and what changed in it, for the passive status line and the
//! session and exit receipts. Everything is read from the store and its
//! journal of committed changes; nothing is estimated and no model is called.

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::CensusEntry;
use codex_project_intelligence::ChangeCount;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::KnowledgeCategory as PiCategory;
use codex_project_intelligence::KnowledgeValidity;

use crate::memory_controls::MemorySection;
use crate::memory_controls::section_of;

/// Identity prefix of commits remembered from the workspace history.
pub(crate) const COMMIT_ID_PREFIX: &str = "stateful-commit-";

/// Current project memory, by what new work does with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryCounts {
    pub rules: u32,
    pub pending_rules: u32,
    pub unverified_rules: u32,
    pub background: u32,
    pub decisions: u32,
    pub open_checks: u32,
    pub commits: u32,
    pub other: u32,
}

/// Committed changes counted over a stretch of the journal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChangeTotals {
    /// New entries saved, commits remembered not included.
    pub saved: u32,
    pub commits_remembered: u32,
    pub promoted: u32,
    pub corrected: u32,
    pub forgotten: u32,
    pub invalidated: u32,
    pub scopes_ended: u32,
    pub capture_incomplete: u32,
}

/// What one active entry counts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tally {
    Section(MemorySection),
    OpenCheck,
    Commit,
}

pub(crate) fn tally(entry: &CensusEntry) -> Option<Tally> {
    // Knowledge no longer current is history, not something memory holds now.
    if matches!(
        entry.validity,
        Some(KnowledgeValidity::Obsolete | KnowledgeValidity::Historical)
    ) {
        return None;
    }
    if entry.category == Some(PiCategory::CommitObservation)
        || entry.id.starts_with(COMMIT_ID_PREFIX)
    {
        return Some(Tally::Commit);
    }
    if entry.category == Some(PiCategory::OpenCheck) || entry.kind == BlackboardKind::Question {
        return Some(Tally::OpenCheck);
    }
    Some(Tally::Section(section_of(
        entry.kind,
        entry.provenance,
        entry.root_promotion,
        &entry.id,
    )))
}

/// Counts the project's current memory.
pub async fn memory_counts(
    store: &BlackboardStore,
    project_id: &str,
) -> Result<MemoryCounts, BlackboardStoreError> {
    Ok(count_census(&store.memory_census(project_id).await?))
}

/// Counts a census of current memory.
pub fn count_census(census: &[CensusEntry]) -> MemoryCounts {
    let mut counts = MemoryCounts::default();
    for entry in census {
        let slot = match tally(entry) {
            None => continue,
            Some(Tally::Commit) => &mut counts.commits,
            Some(Tally::OpenCheck) => &mut counts.open_checks,
            Some(Tally::Section(MemorySection::UserRule)) => &mut counts.rules,
            Some(Tally::Section(MemorySection::PendingRule)) => &mut counts.pending_rules,
            Some(Tally::Section(MemorySection::UnverifiedRule)) => &mut counts.unverified_rules,
            Some(Tally::Section(MemorySection::Background)) => &mut counts.background,
            Some(Tally::Section(MemorySection::Decision)) => &mut counts.decisions,
            Some(Tally::Section(MemorySection::Knowledge)) => &mut counts.other,
        };
        *slot += 1;
    }
    counts
}

/// Sums journal counts into totals; a remembered commit is not counted as a save.
pub fn change_totals(counts: &[ChangeCount]) -> ChangeTotals {
    let mut totals = ChangeTotals::default();
    for count in counts {
        let slot = match count.operation {
            ChangeOperation::Saved if count.category == PiCategory::CommitObservation => {
                &mut totals.commits_remembered
            }
            ChangeOperation::Saved => &mut totals.saved,
            ChangeOperation::Promoted => &mut totals.promoted,
            ChangeOperation::Corrected => &mut totals.corrected,
            ChangeOperation::Forgotten => &mut totals.forgotten,
            ChangeOperation::Invalidated => &mut totals.invalidated,
            ChangeOperation::ScopeEnded => &mut totals.scopes_ended,
            ChangeOperation::CaptureIncomplete => &mut totals.capture_incomplete,
        };
        *slot = slot.saturating_add(count.count);
    }
    totals
}

#[cfg(test)]
#[path = "memory_activity_tests.rs"]
mod tests;
