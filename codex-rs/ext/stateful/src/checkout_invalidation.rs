//! Remembered conclusions that observed commits may have made untrue.
//!
//! When HEAD advanced between Stateful turns, the host reads each new commit's patch (within
//! an aggregate byte budget and a deadline) and keeps the keyed values it rebound. Remembered
//! statements and intentions that may state an old value (filtered before any cap) are marked
//! as needing a check, with the commit as the reason; they stay readable and are never
//! retired on this evidence, because the host does not prove what the code now does. Commit
//! facts are history and never judged; neither are the user's own words.
//!
//! Only current entries are candidates, so a pass that runs out of time (or fails a write)
//! keeps the checkout baseline and the next turn resumes with the entries not yet marked.
//! Work that can never finish here (too many commits, patch bytes, changes or candidates) is
//! reported and journaled as incomplete. Every write is revision-guarded; an entry changed
//! concurrently is judged again from its new revision, never overwritten.

use codex_git_utils::GitCommitSummary;
use codex_git_utils::GitObservationBudget;
use codex_git_utils::GitObservationFailure;
use codex_git_utils::commit_patch;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::InvalidationCandidates;
use codex_project_intelligence::InvalidationOutcome;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeValidity;
use codex_protocol::protocol::GitSha;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::json;
use tokio::time::Instant;

use crate::code_anchors::LiteralChange;
use crate::code_anchors::literal_changes;
use crate::code_anchors::may_state_old_value;
use crate::services::ProjectIntelligenceServices;

/// Commits examined per comparison.
pub(crate) const MAX_EXAMINED_COMMITS: usize = 32;
/// Patch bytes examined per comparison, across all commits.
const MAX_PATCH_BYTES: usize = 256 * 1024;
/// Literal changes examined per comparison, across all commits.
const MAX_CHANGES: usize = 32;
/// Remembered entries examined per change.
const MAX_CANDIDATES: u32 = 200;
/// Kinds of remembered statements and intentions a code change can concern.
const JUDGED_KINDS: &[BlackboardKind] = &[
    BlackboardKind::Fact,
    BlackboardKind::Claim,
    BlackboardKind::Number,
    BlackboardKind::Note,
    BlackboardKind::Decision,
    BlackboardKind::Strategy,
];

/// The commits that advanced HEAD in one root.
pub(crate) struct AdvancedRoot<'a> {
    pub(crate) worktree_root: &'a AbsolutePathBuf,
    /// Newest first, at most `MAX_EXAMINED_COMMITS`.
    pub(crate) commits: &'a [GitCommitSummary],
    /// More commits advanced HEAD than were listed.
    pub(crate) omitted: bool,
}

/// What one pass did: report lines for the turn, and whether the checkout baseline must be
/// kept so the next turn resumes (the pass stopped early or a write failed).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CheckPass {
    pub(crate) lines: Vec<String>,
    pub(crate) retry: bool,
}

/// One literal change and the commit that made it.
struct CommittedChange<'a> {
    change: LiteralChange,
    commit: &'a GitCommitSummary,
}

/// Marks remembered conclusions the commits of `advanced` may have made untrue as needing a
/// check. Stops at `deadline`.
pub(crate) async fn qualify_changed_conclusions(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    advanced: &AdvancedRoot<'_>,
    budget: &GitObservationBudget,
    deadline: Instant,
) -> CheckPass {
    let mut pass = CheckPass::default();
    let mut unexamined = Vec::new();
    if advanced.omitted {
        unexamined.push(format!(
            "commits older than the newest {MAX_EXAMINED_COMMITS}"
        ));
    }
    let changes = committed_changes(advanced, budget, deadline, &mut pass, &mut unexamined).await;
    if !changes.is_empty() {
        match services.blackboard().await {
            Ok(store) => {
                let judge = Judge { store, project_id };
                for checked in &changes {
                    if Instant::now() >= deadline {
                        pass.retry = true;
                        break;
                    }
                    judge
                        .qualify(checked, deadline, &mut pass, &mut unexamined)
                        .await;
                }
            }
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to open the blackboard for invalidation");
                pass.retry = true;
            }
        }
    }
    if !unexamined.is_empty() {
        let preview = format!(
            "Remembered conclusions were not checked against every change up to commit {}: {}.",
            advanced
                .commits
                .first()
                .map_or("", |commit| short(&commit.oid)),
            unexamined.join("; "),
        );
        let record = change_record(
            ChangeOperation::CaptureIncomplete,
            KnowledgeCategory::CommitObservation,
            preview.clone(),
        );
        let journaled = match services.blackboard().await {
            Ok(store) => store.record_change(project_id, None, &record).await.is_ok(),
            Err(_) => false,
        };
        if !journaled {
            tracing::warn!(%project_id, "failed to journal an incomplete invalidation");
            pass.retry = true;
        }
        pass.lines.push(format!(
            "{preview} Remembered code facts may be out of date; check the source."
        ));
    }
    pass
}

/// The keyed literal changes of the commits, oldest first.
async fn committed_changes<'a>(
    advanced: &AdvancedRoot<'a>,
    budget: &GitObservationBudget,
    deadline: Instant,
    pass: &mut CheckPass,
    unexamined: &mut Vec<String>,
) -> Vec<CommittedChange<'a>> {
    let mut examined_bytes = 0usize;
    let mut changes = Vec::new();
    for commit in advanced.commits.iter().rev() {
        if Instant::now() >= deadline {
            pass.retry = true;
            break;
        }
        let patch =
            match commit_patch(advanced.worktree_root, &GitSha::new(&commit.oid), budget).await {
                Ok(patch) => patch,
                Err(GitObservationFailure::Timeout) => {
                    pass.retry = true;
                    break;
                }
                Err(error) => {
                    tracing::warn!(?error, "failed to read an observed commit's patch");
                    unexamined.push(format!("commit {} and later", short(&commit.oid)));
                    break;
                }
            };
        examined_bytes += patch.len();
        if examined_bytes > MAX_PATCH_BYTES {
            unexamined.push(format!(
                "commit {} and later (more than {MAX_PATCH_BYTES} patch bytes)",
                short(&commit.oid)
            ));
            break;
        }
        let (found, whole) = literal_changes(&patch);
        if !whole || changes.len() + found.len() > MAX_CHANGES {
            unexamined.push(format!(
                "part of commit {} (more than {MAX_CHANGES} changed values)",
                short(&commit.oid)
            ));
        }
        let room = MAX_CHANGES - changes.len();
        changes.extend(
            found
                .into_iter()
                .take(room)
                .map(|change| CommittedChange { change, commit }),
        );
    }
    changes
}

/// Whether the host may judge `entry`: active, not the user's own words, not a host record
/// of what happened, and of a judged kind.
fn eligible(entry: &BlackboardEntry) -> bool {
    entry.state == BlackboardEntryState::Active
        && !matches!(
            entry.value.provenance.kind,
            BlackboardProvenanceKind::User | BlackboardProvenanceKind::Maintenance
        )
        && JUDGED_KINDS.contains(&entry.value.kind)
}

/// What recording one entry did.
#[derive(Debug, PartialEq, Eq)]
enum Recorded {
    Marked,
    Unchanged,
    /// Left for the next pass, which the caller must schedule.
    Deferred,
}

struct Judge<'a> {
    store: &'a BlackboardStore,
    project_id: &'a str,
}

impl Judge<'_> {
    /// Marks the current entries that may state the old value of `checked`.
    async fn qualify(
        &self,
        checked: &CommittedChange<'_>,
        deadline: Instant,
        pass: &mut CheckPass,
        unexamined: &mut Vec<String>,
    ) {
        let candidates = self
            .store
            .invalidation_candidates(
                self.project_id,
                InvalidationCandidates {
                    kinds: JUDGED_KINDS,
                    mentioning: &checked.change.old,
                    limit: MAX_CANDIDATES,
                },
            )
            .await;
        let (candidates, more) = match candidates {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::warn!(project_id = %self.project_id, %error, "failed to list entries for invalidation");
                pass.retry = true;
                return;
            }
        };
        if more {
            unexamined.push(format!(
                "entries beyond the newest {MAX_CANDIDATES} that mention {}",
                quote(&checked.change.old)
            ));
        }
        let mut marked = 0usize;
        for entry in candidates {
            if Instant::now() >= deadline {
                pass.retry = true;
                break;
            }
            if !eligible(&entry) || !may_state_old_value(&entry.value.content, &checked.change) {
                continue;
            }
            match self.mark(entry, checked).await {
                Ok(Recorded::Marked) => marked += 1,
                Ok(Recorded::Unchanged) => {}
                Ok(Recorded::Deferred) => pass.retry = true,
                Err(error) => {
                    tracing::warn!(project_id = %self.project_id, %error, "failed to mark a remembered conclusion");
                    pass.retry = true;
                }
            }
        }
        if marked > 0 {
            pass.lines.push(format!(
                "{marked} remembered conclusion(s) may be outdated: {}.",
                reason(checked)
            ));
        }
    }

    /// Marks `entry` as needing a check. If another writer changed it first, the new revision
    /// is judged again once; a second conflict is left for the next pass.
    async fn mark(
        &self,
        mut entry: BlackboardEntry,
        checked: &CommittedChange<'_>,
    ) -> Result<Recorded, BlackboardStoreError> {
        for attempt in 0..2 {
            let context = self
                .store
                .knowledge_context_at(self.project_id, &entry.id, entry.revision)
                .await?;
            if context
                .as_ref()
                .is_some_and(|context| context.validity != KnowledgeValidity::Current)
            {
                return Ok(Recorded::Unchanged);
            }
            let category = context
                .as_ref()
                .map_or(KnowledgeCategory::Legacy, |context| context.category);
            let next = crate::knowledge_validity::invalidated_context(
                context,
                KnowledgeValidity::NeedsCheck,
                json!({ "reason": reason(checked), "commit": checked.commit.oid }),
            );
            let record = change_record(
                ChangeOperation::Invalidated,
                category,
                entry.value.content.clone(),
            );
            match self
                .store
                .invalidate_entry(self.project_id, &entry.id, entry.revision, next, &record)
                .await
            {
                Ok(InvalidationOutcome::Invalidated(_)) => return Ok(Recorded::Marked),
                Ok(InvalidationOutcome::Unchanged) => return Ok(Recorded::Unchanged),
                Err(BlackboardStoreError::RevisionConflict { .. }) if attempt == 0 => {
                    let Some(reloaded) = self.store.get_entry(self.project_id, &entry.id).await?
                    else {
                        return Ok(Recorded::Unchanged);
                    };
                    if !eligible(&reloaded)
                        || !may_state_old_value(&reloaded.value.content, &checked.change)
                    {
                        return Ok(Recorded::Unchanged);
                    }
                    entry = reloaded;
                }
                Err(BlackboardStoreError::RevisionConflict { .. }) => {
                    return Ok(Recorded::Deferred);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(Recorded::Deferred)
    }
}

fn reason(checked: &CommittedChange<'_>) -> String {
    let CommittedChange { change, commit } = checked;
    format!(
        "commit {} changed {} {} from {} to {}; check the current source before relying on it",
        short(&commit.oid),
        change.path,
        change.key,
        quote(&change.old),
        quote(&change.new),
    )
}

fn change_record(
    operation: ChangeOperation,
    category: KnowledgeCategory,
    preview: String,
) -> ChangeRecord {
    ChangeRecord {
        operation,
        origin: ChangeOrigin::HostObserved,
        category,
        action_id: None,
        thread_id: None,
        turn_id: None,
        group_id: None,
        preview,
    }
}

fn short(oid: &str) -> &str {
    &oid[..oid.len().min(8)]
}

fn quote(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

#[cfg(test)]
#[path = "checkout_invalidation_tests.rs"]
mod tests;
