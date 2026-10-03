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
    /// Whether the history read could establish the last finished work: every recent thread
    /// was readable, and the bounded read found finished work or saw all there was.
    pub history_complete: bool,
}

/// What the recap may show as current here: not obsolete, historical, needing a check,
/// stale or disputed, and project-wide or limited to the open investigation of this thread.
struct Applicability {
    bound_scope: Option<String>,
}

/// How many times an entry is read again when it changes while being read.
const READ_ATTEMPTS: usize = 3;

impl Applicability {
    fn census(&self, entry: &CensusEntry) -> bool {
        current(entry.validity, entry.verification)
            && entry
                .scope_id
                .as_ref()
                .is_none_or(|scope| self.bound_scope.as_ref() == Some(scope))
    }

    /// Reads the entry's current revision together with the context of that same revision,
    /// and checks again that it is still what was selected and still applies here.
    async fn load(
        &self,
        store: &BlackboardStore,
        project_id: &str,
        id: &str,
        wanted: Tally,
    ) -> Result<Option<(BlackboardEntry, Option<KnowledgeAuthority>)>, BlackboardStoreError> {
        let Ok(id) = BlackboardEntryId::parse(id.to_string()) else {
            return Ok(None);
        };
        for _ in 0..READ_ATTEMPTS {
            let Some(entry) = store.get_entry(project_id, &id).await? else {
                return Ok(None);
            };
            let context = store.knowledge_context(project_id, &id).await?;
            // The context belongs to the revision read only if no revision came in between.
            let unchanged = store
                .get_entry(project_id, &id)
                .await?
                .is_some_and(|again| again.revision == entry.revision);
            if !unchanged {
                continue;
            }
            let reread = CensusEntry {
                id: entry.id.to_string(),
                kind: entry.value.kind,
                provenance: entry.value.provenance.kind,
                root_promotion: entry.value.root_promotion,
                category: context.as_ref().map(|context| context.category),
                validity: context.as_ref().map(|context| context.validity),
                authority: context.as_ref().map(|context| context.authority),
                verification: entry.value.verification,
                scope_id: context
                    .as_ref()
                    .and_then(|context| context.scope_id.clone()),
                source_sequence: None,
                unit_ordinal: None,
                created_at_ms: 0,
                updated_at_ms: entry.updated_at_ms,
            };
            let applies = entry.state == BlackboardEntryState::Active
                && tally(&reread) == Some(wanted)
                && self.census(&reread);
            return Ok(applies.then_some((entry, reread.authority)));
        }
        Ok(None)
    }
}

fn current(validity: Option<KnowledgeValidity>, verification: BlackboardVerification) -> bool {
    matches!(validity, None | Some(KnowledgeValidity::Current))
        && !matches!(
            verification,
            BlackboardVerification::Stale | BlackboardVerification::Disputed
        )
}

/// What one list of the recap shows, and how many selected entries no longer applied when
/// read again (so they are not announced as more).
struct Shown {
    items: Vec<(BlackboardEntry, Option<KnowledgeAuthority>)>,
    dropped: usize,
}

impl Shown {
    fn more(&self, total: usize) -> u32 {
        u32::try_from(total.saturating_sub(self.items.len() + self.dropped)).unwrap_or(u32::MAX)
    }
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
    // The bounded history read may have missed the latest finished work when it found none
    // in what it read but more history exists.
    let history_complete = !continuity.history_unavailable
        && continuity.unreadable_threads == 0
        && (last_work.is_some() || !continuity.more_turns);
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
            .collect::<Vec<_>>()
    };
    // Rules keep the order the user stated them (capture position, then storage order for
    // rules without one), as new work receives them.
    let mut rule_entries = eligible(Tally::Section(MemorySection::UserRule));
    rule_entries.sort_by(|left, right| {
        (
            left.source_sequence,
            left.unit_ordinal,
            left.created_at_ms,
            &left.id,
        )
            .cmp(&(
                right.source_sequence,
                right.unit_ordinal,
                right.created_at_ms,
                &right.id,
            ))
    });
    let ids = |entries: Vec<&CensusEntry>| {
        entries
            .into_iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>()
    };
    let rule_ids = ids(rule_entries);
    let decision_ids = ids(eligible(Tally::Section(MemorySection::Decision)));
    let open_check_ids = ids(eligible(Tally::OpenCheck));
    let commit_ids = ids(eligible(Tally::Commit));

    let rules = shown(
        store,
        project_id,
        &applicability,
        &rule_ids,
        Tally::Section(MemorySection::UserRule),
        MAX_RECAP_RULES,
    )
    .await?;
    let decisions = shown(
        store,
        project_id,
        &applicability,
        &decision_ids,
        Tally::Section(MemorySection::Decision),
        MAX_RECAP_ITEMS,
    )
    .await?;
    let open_checks = shown(
        store,
        project_id,
        &applicability,
        &open_check_ids,
        Tally::OpenCheck,
        MAX_RECAP_ITEMS,
    )
    .await?;
    let commits = shown(
        store,
        project_id,
        &applicability,
        &commit_ids,
        Tally::Commit,
        MAX_RECAP_ITEMS,
    )
    .await?;
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

    Ok(ReturnRecap {
        as_of_ms: continuity.captured_at_ms,
        last_work,
        more_rules: rules.more(rule_ids.len()),
        rules: rules
            .items
            .iter()
            .map(|(entry, _)| bounded(&entry.value.content))
            .collect(),
        more_decisions: decisions.more(decision_ids.len()),
        decisions: decisions
            .items
            .iter()
            .map(|(entry, authority)| {
                let reported = entry.value.provenance.kind != BlackboardProvenanceKind::User
                    || *authority == Some(KnowledgeAuthority::AssistantReported);
                decision(&entry.value.content, reported)
            })
            .collect(),
        more_open_checks: open_checks.more(open_check_ids.len()),
        open_checks: open_checks
            .items
            .iter()
            .map(|(entry, _)| bounded(&entry.value.content))
            .collect(),
        more_commits: commits.more(commit_ids.len()),
        commits: commits
            .items
            .iter()
            .map(|(entry, _)| commit_line(&entry.value.content))
            .collect(),
        capture_incomplete,
        history_complete,
    })
}

/// The first `limit` of `ids` that are still `wanted` and still apply when read again.
async fn shown(
    store: &BlackboardStore,
    project_id: &str,
    applicability: &Applicability,
    ids: &[String],
    wanted: Tally,
    limit: usize,
) -> Result<Shown, BlackboardStoreError> {
    let mut shown = Shown {
        items: Vec::new(),
        dropped: 0,
    };
    for id in ids {
        if shown.items.len() == limit {
            break;
        }
        match applicability.load(store, project_id, id, wanted).await? {
            Some(found) => shown.items.push(found),
            None => shown.dropped += 1,
        }
    }
    Ok(shown)
}

/// A commit observation as `<short sha>: <subject>` when its current text is still the
/// stored fact ("Commit <hex id> (committed ...) ...): <subject> <body>"); any other text,
/// such as the user's correction, is shown as written.
fn commit_line(content: &str) -> String {
    let parsed = content
        .strip_prefix("Commit ")
        .and_then(|rest| rest.split_once(' '))
        .filter(|(sha, _)| sha.len() >= 7 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
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
