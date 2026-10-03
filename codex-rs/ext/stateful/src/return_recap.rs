//! The dated return card: the last finished work, the rules that apply in the thread, current
//! decisions with their recorded reasons, open checks, the commits most recently remembered
//! and capture gaps since the last finished work. Assembled from stored memory with no model
//! call; nothing is invented, and only knowledge that is current and applies here is shown.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::CensusEntry;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::ScopeState;
use codex_thread_store::ThreadStore;

use crate::memory_activity::Tally;
use crate::memory_activity::change_totals;
use crate::memory_activity::tally;
use crate::memory_controls::MemorySection;

/// Rules, and decisions, open checks and commits, listed by a recap.
const MAX_RECAP_RULES: usize = 5;
const MAX_RECAP_ITEMS: usize = 3;
/// Longest text of one recap line, in bytes.
const MAX_RECAP_TEXT_BYTES: usize = 240;
/// Rules read for a recap's order (the packet's own projection bound).
const RECAP_PROJECTION_ENTRIES: u32 = 256;

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
    /// The assistant's conclusion, not the user's word.
    pub reported: bool,
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
    /// Commits still remembered, most recently remembered first.
    pub commits: Vec<String>,
    pub more_commits: u32,
    /// Captures that could not finish since the last finished work. Gaps from before it are
    /// not assessed.
    pub capture_incomplete: u32,
    /// Whether every recent thread's history could be read for the last finished work.
    pub history_complete: bool,
}

/// What the recap may show as current here: not obsolete, historical, needing a check,
/// stale or disputed, and project-wide or limited to the open investigation of this thread.
struct Applicability {
    bound_scope: Option<String>,
}

impl Applicability {
    fn census(&self, entry: &CensusEntry) -> bool {
        current(entry.validity, entry.verification)
            && entry
                .scope_id
                .as_ref()
                .is_none_or(|scope| self.bound_scope.as_ref() == Some(scope))
    }

    /// Reads the entry's current revision and checks it again: it may have changed since
    /// the census.
    async fn load(
        &self,
        store: &BlackboardStore,
        project_id: &str,
        id: &str,
    ) -> Result<Option<(BlackboardEntry, Option<KnowledgeAuthority>)>, BlackboardStoreError> {
        let Ok(id) = BlackboardEntryId::parse(id.to_string()) else {
            return Ok(None);
        };
        let Some(entry) = store.get_entry(project_id, &id).await? else {
            return Ok(None);
        };
        let context = store.knowledge_context(project_id, &id).await?;
        let applies = entry.state == BlackboardEntryState::Active
            && current(
                context.as_ref().map(|context| context.validity),
                entry.value.verification,
            )
            && context
                .as_ref()
                .and_then(|context| context.scope_id.as_ref())
                .is_none_or(|scope| self.bound_scope.as_ref() == Some(scope));
        Ok(applies.then(|| (entry, context.map(|context| context.authority))))
    }
}

fn current(validity: Option<KnowledgeValidity>, verification: BlackboardVerification) -> bool {
    matches!(validity, None | Some(KnowledgeValidity::Current))
        && !matches!(
            verification,
            BlackboardVerification::Stale | BlackboardVerification::Disputed
        )
}

/// Assembles the return recap for `thread_id`.
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
    let history_complete = !continuity.history_unavailable && continuity.unreadable_threads == 0;
    let applicability = Applicability {
        bound_scope: store
            .thread_scope(project_id, thread_id)
            .await?
            .filter(|scope| scope.state == ScopeState::Open)
            .map(|scope| scope.scope_id),
    };
    // Filtered before anything is cut, so entries that do not apply never take a place.
    let mut census = store.memory_census(project_id).await?;
    census.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at_ms));
    let eligible = |wanted: Tally| {
        census
            .iter()
            .filter(|entry| tally(entry) == Some(wanted) && applicability.census(entry))
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>()
    };
    let rule_ids = eligible(Tally::Section(MemorySection::UserRule));
    let decision_ids = eligible(Tally::Section(MemorySection::Decision));
    let open_check_ids = eligible(Tally::OpenCheck);
    let commit_ids = eligible(Tally::Commit);

    // Rules keep the order the user stated them, as new work receives them; any the
    // packet's bounded projection did not reach follow, newest first.
    let projection = store
        .root_projection(RootBlackboardQuery {
            project_id: project_id.to_string(),
            max_entries: RECAP_PROJECTION_ENTRIES,
        })
        .await?;
    let mut ordered_rules = projection
        .data
        .iter()
        .map(|hit| hit.entry.id.to_string())
        .filter(|id| rule_ids.contains(id))
        .collect::<Vec<_>>();
    for id in &rule_ids {
        if !ordered_rules.contains(id) {
            ordered_rules.push(id.clone());
        }
    }
    let rules = shown(
        store,
        project_id,
        &applicability,
        &ordered_rules,
        MAX_RECAP_RULES,
    )
    .await?
    .into_iter()
    .map(|(entry, _)| bounded(&entry.value.content))
    .collect::<Vec<_>>();
    let decisions = shown(
        store,
        project_id,
        &applicability,
        &decision_ids,
        MAX_RECAP_ITEMS,
    )
    .await?
    .into_iter()
    .map(|(entry, authority)| {
        let reported = entry.value.provenance.kind != BlackboardProvenanceKind::User
            || authority == Some(KnowledgeAuthority::AssistantReported);
        decision(&entry.value.content, reported)
    })
    .collect::<Vec<_>>();
    let open_checks = shown(
        store,
        project_id,
        &applicability,
        &open_check_ids,
        MAX_RECAP_ITEMS,
    )
    .await?
    .into_iter()
    .map(|(entry, _)| bounded(&entry.value.content))
    .collect::<Vec<_>>();
    let commits = shown(
        store,
        project_id,
        &applicability,
        &commit_ids,
        MAX_RECAP_ITEMS,
    )
    .await?
    .into_iter()
    .map(|(entry, _)| commit_line(&entry.value.content))
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

    let more =
        |total: usize, shown: usize| u32::try_from(total.saturating_sub(shown)).unwrap_or(u32::MAX);
    Ok(ReturnRecap {
        as_of_ms: continuity.captured_at_ms,
        last_work,
        more_rules: more(rule_ids.len(), rules.len()),
        rules,
        more_decisions: more(decision_ids.len(), decisions.len()),
        decisions,
        more_open_checks: more(open_check_ids.len(), open_checks.len()),
        open_checks,
        more_commits: more(commit_ids.len(), commits.len()),
        commits,
        capture_incomplete,
        history_complete,
    })
}

/// The first `limit` of `ids` that still apply when read again.
async fn shown(
    store: &BlackboardStore,
    project_id: &str,
    applicability: &Applicability,
    ids: &[String],
    limit: usize,
) -> Result<Vec<(BlackboardEntry, Option<KnowledgeAuthority>)>, BlackboardStoreError> {
    let mut shown = Vec::new();
    for id in ids {
        if shown.len() == limit {
            break;
        }
        if let Some(found) = applicability.load(store, project_id, id).await? {
            shown.push(found);
        }
    }
    Ok(shown)
}

/// A commit observation as `<short sha>: <subject>`, from the stored fact's current text
/// ("Commit <sha> (committed ...) ...: <subject> <body>"); other text is shown as written.
fn commit_line(content: &str) -> String {
    let parsed = content
        .strip_prefix("Commit ")
        .and_then(|rest| rest.split_once(' '))
        .zip(content.split_once("): "))
        .map(|((sha, _), (_, subject))| format!("{}: {subject}", &sha[..sha.len().min(8)]));
    bounded(parsed.as_deref().unwrap_or(content))
}

/// A decision's text and the reason recorded with it ("... Reason: ...").
fn decision(content: &str, reported: bool) -> RecapDecision {
    match content.split_once(" Reason: ") {
        Some((text, reason)) if !reason.trim().is_empty() => RecapDecision {
            text: bounded(text),
            reason: Some(bounded(reason.trim())),
            reported,
        },
        Some(_) | None => RecapDecision {
            text: bounded(content),
            reason: None,
            reported,
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
#[path = "return_recap_tests.rs"]
mod tests;
