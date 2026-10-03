//! What project memory holds and what changed in it, for the passive status line, session and
//! exit receipts, and the dated return recap. Everything is read from the store and its
//! journal of committed changes; nothing is estimated and no model is called.

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::CensusEntry;
use codex_project_intelligence::ChangeCount;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::KnowledgeCategory as PiCategory;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::RootBlackboardQuery;
use codex_thread_store::ThreadStore;

use crate::memory_controls::MemorySection;
use crate::memory_controls::section_of;
use crate::rule_scope::ScopeView;

/// Identity prefix of commits remembered from the workspace history.
pub(crate) const COMMIT_ID_PREFIX: &str = "stateful-commit-";
/// Rules, decisions, open checks and commits listed by a recap.
const MAX_RECAP_RULES: usize = 5;
const MAX_RECAP_ITEMS: usize = 3;
/// Longest text of one recap line, in bytes.
const MAX_RECAP_TEXT_BYTES: usize = 240;
/// Rules read for a recap (the packet's own projection bound).
const RECAP_PROJECTION_ENTRIES: u32 = 256;

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

/// The last finished piece of work in the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecapWork {
    pub thread_id: String,
    pub finished_at_ms: i64,
    pub request: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecapDecision {
    pub text: String,
    pub reason: Option<String>,
}

/// A dated return card from stored memory; every list is bounded and says how much it left
/// out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReturnRecap {
    pub as_of_ms: i64,
    pub last_work: Option<RecapWork>,
    pub rules: Vec<String>,
    pub more_rules: u32,
    pub decisions: Vec<RecapDecision>,
    pub more_decisions: u32,
    pub open_checks: Vec<String>,
    pub more_open_checks: u32,
    /// Commits most recently remembered from the workspace history, newest first.
    pub commits: Vec<String>,
    pub more_commits: u32,
    /// Captures that could not finish since the last finished work.
    pub capture_incomplete: u32,
}

/// What one active entry counts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tally {
    Section(MemorySection),
    OpenCheck,
    Commit,
}

fn tally(entry: &CensusEntry) -> Option<Tally> {
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
    let mut counts = MemoryCounts::default();
    for entry in store.memory_census(project_id).await? {
        let slot = match tally(&entry) {
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
    Ok(counts)
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

/// Assembles the return recap for `thread_id`: the last finished work, the rules that apply
/// in this thread, current decisions with their recorded reasons, open checks, recently
/// remembered commits and capture gaps. No next action is invented.
pub async fn return_recap(
    store: &BlackboardStore,
    threads: &dyn ThreadStore,
    project_id: &str,
    thread_id: &str,
) -> Result<ReturnRecap, BlackboardStoreError> {
    let continuity = crate::continuity_source::gather_continuity(
        threads, /*runtime*/ None, project_id, thread_id,
    )
    .await;
    let last_work = continuity
        .turns
        .iter()
        .find(|turn| turn.unfinished_status.is_none() && turn.at_ms.is_some())
        .map(|turn| RecapWork {
            thread_id: turn.thread_id.clone(),
            finished_at_ms: turn.at_ms.unwrap_or_default(),
            request: turn.user.as_deref().map(bounded),
        });

    let mut projection = store
        .root_projection(RootBlackboardQuery {
            project_id: project_id.to_string(),
            max_entries: RECAP_PROJECTION_ENTRIES,
        })
        .await?;
    ScopeView::load(store, &projection, thread_id)
        .await
        .retain_applicable(&mut projection);
    let rules = projection
        .data
        .iter()
        .map(|hit| &hit.entry)
        .filter(|entry| {
            section_of(
                entry.value.kind,
                entry.value.provenance.kind,
                entry.value.root_promotion,
                entry.id.as_str(),
            ) == MemorySection::UserRule
        })
        .map(|entry| bounded(&entry.value.content))
        .collect::<Vec<_>>();

    let mut census = store.memory_census(project_id).await?;
    census.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at_ms));
    let decision_ids = census
        .iter()
        .filter(|entry| tally(entry) == Some(Tally::Section(MemorySection::Decision)))
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let open_check_ids = census
        .iter()
        .filter(|entry| tally(entry) == Some(Tally::OpenCheck))
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let mut decisions = Vec::new();
    for id in decision_ids.iter().take(MAX_RECAP_ITEMS) {
        if let Some(entry) = load(store, project_id, id).await? {
            decisions.push(decision(&entry.value.content));
        }
    }
    let mut open_checks = Vec::new();
    for id in open_check_ids.iter().take(MAX_RECAP_ITEMS) {
        if let Some(entry) = load(store, project_id, id).await? {
            open_checks.push(bounded(&entry.value.content));
        }
    }
    let commit_total = census
        .iter()
        .filter(|entry| tally(entry) == Some(Tally::Commit))
        .count();
    let commits = store
        .recent_changes(
            project_id,
            ChangeOperation::Saved,
            PiCategory::CommitObservation,
            u32::try_from(MAX_RECAP_ITEMS).unwrap_or(u32::MAX),
        )
        .await?
        .into_iter()
        .map(|change| {
            bounded(
                change
                    .record
                    .preview
                    .strip_prefix("Remembered commit ")
                    .unwrap_or(&change.record.preview),
            )
        })
        .collect::<Vec<_>>();
    let gaps_after = match &last_work {
        Some(work) => store.sequence_at(project_id, work.finished_at_ms).await?,
        None => 0,
    };
    let capture_incomplete = change_totals(
        &store
            .change_totals(project_id, gaps_after, /*thread_ids*/ None)
            .await?,
    )
    .capture_incomplete;

    let more = |total: usize, shown: usize| u32::try_from(total - shown).unwrap_or(u32::MAX);
    let rule_count = rules.len();
    Ok(ReturnRecap {
        as_of_ms: continuity.captured_at_ms,
        last_work,
        more_rules: more(rule_count, rule_count.min(MAX_RECAP_RULES)),
        rules: rules.into_iter().take(MAX_RECAP_RULES).collect(),
        more_decisions: more(decision_ids.len(), decisions.len()),
        decisions,
        more_open_checks: more(open_check_ids.len(), open_checks.len()),
        open_checks,
        more_commits: more(commit_total.max(commits.len()), commits.len()),
        commits,
        capture_incomplete,
    })
}

async fn load(
    store: &BlackboardStore,
    project_id: &str,
    id: &str,
) -> Result<Option<codex_project_intelligence::BlackboardEntry>, BlackboardStoreError> {
    let Ok(id) = codex_project_intelligence::BlackboardEntryId::parse(id.to_string()) else {
        return Ok(None);
    };
    store.get_entry(project_id, &id).await
}

/// A decision's text and the reason recorded with it ("... Reason: ...").
fn decision(content: &str) -> RecapDecision {
    match content.split_once(" Reason: ") {
        Some((text, reason)) if !reason.trim().is_empty() => RecapDecision {
            text: bounded(text),
            reason: Some(bounded(reason.trim())),
        },
        Some(_) | None => RecapDecision {
            text: bounded(content),
            reason: None,
        },
    }
}

/// One line of `text`, at most `MAX_RECAP_TEXT_BYTES`, cut on a character boundary and
/// marked when cut.
fn bounded(text: &str) -> String {
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.len() <= MAX_RECAP_TEXT_BYTES {
        return single;
    }
    let mut end = MAX_RECAP_TEXT_BYTES - '\u{2026}'.len_utf8();
    while !single.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &single[..end])
}

#[cfg(test)]
#[path = "memory_activity_tests.rs"]
mod tests;
