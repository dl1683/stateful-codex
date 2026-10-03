//! Remembered assertions that committed source changes may have made untrue are marked as
//! needing a check, file by file.
//!
//! Per project root the host keeps a qualification job: the last commit every eligible
//! assertion was checked against (the qualified head), the target commit, the paths that
//! changed between them, and a scan cursor. Nothing is parsed: an agent assertion whose node or
//! evidence is a changed file (or lies below a changed directory) is marked with the path and
//! the commit range as the reason; an agent observation that names no source file is marked
//! whenever any file changed, since nothing shows what it depends on. Nothing is deleted,
//! rewritten or asserted about the new code, and the user's own words are never judged.
//!
//! All work of a turn shares one deadline. Each mark and each cursor step commits on its own,
//! so a stopped pass resumes where it was; only a complete scan advances the qualified head.
//! When the changes cannot be listed (history moved discontinuously, or more than the path
//! budget changed), the job is blocked for that target and is not retried until HEAD moves:
//! readers then treat everything recorded before the target as unchecked.

use std::future::Future;
use std::path::Path;

use codex_git_utils::GitCommitRange;
use codex_git_utils::GitObservationBudget;
use codex_git_utils::commits_between;
use codex_git_utils::paths_changed_between;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::InvalidationOutcome;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::QualificationJob;
use codex_project_intelligence::QualificationState;
use codex_project_intelligence::ScanItem;
use codex_project_intelligence::SourceDependency;
use codex_protocol::protocol::GitSha;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::json;
use tokio::time::Instant;

use crate::services::ProjectIntelligenceServices;

/// Changed paths kept in one manifest; more is unknown coverage, never a truncated list.
const MAX_CHANGED_PATHS: usize = 1_024;
/// Entries read per scan page.
const PAGE_ENTRIES: u32 = 64;
/// Entries examined per turn.
const MAX_EXAMINED_PER_TURN: usize = 256;
/// Time kept at the end of the deadline for committing scan progress.
const PROGRESS_RESERVE: std::time::Duration = std::time::Duration::from_millis(250);
/// Kinds stating what the code does; without a recorded source they are judged by any change.
pub(crate) const OBSERVATION_KINDS: &[BlackboardKind] = &[
    BlackboardKind::Fact,
    BlackboardKind::Claim,
    BlackboardKind::Number,
];

/// One project root as the checkout observation found it.
pub(crate) struct ObservedRoot<'a> {
    pub(crate) project_root: &'a str,
    pub(crate) worktree_root: &'a AbsolutePathBuf,
    pub(crate) head: &'a str,
    /// The head of the previous observation, when there was one.
    pub(crate) previous_head: Option<&'a str>,
}

/// Runs `work` unless `deadline` has passed first; `None` means the deadline won.
async fn before<T>(deadline: Instant, work: impl Future<Output = T>) -> Option<T> {
    if Instant::now() >= deadline {
        return None;
    }
    tokio::time::timeout_at(deadline, work).await.ok()
}

/// Starts, replaces or continues the qualification of `root` within `deadline`. Returns report
/// lines for the turn.
pub(crate) async fn qualify_root(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    root: &ObservedRoot<'_>,
    deadline: Instant,
) -> Vec<String> {
    let Ok(store) = services.blackboard().await else {
        return Vec::new();
    };
    let (job, started) = match before(deadline, job_for(store, project_id, root, deadline)).await {
        Some(Ok(job)) => job,
        Some(Err(error)) => {
            tracing::warn!(%project_id, %error, "failed to prepare source qualification");
            return Vec::new();
        }
        None => return Vec::new(),
    };
    match job.state {
        QualificationState::Complete => Vec::new(),
        QualificationState::Blocked if started => vec![blocked_line(&job)],
        QualificationState::Blocked => Vec::new(),
        QualificationState::Scanning => {
            let manifest = job
                .manifest
                .as_deref()
                .and_then(|manifest| serde_json::from_str::<Vec<String>>(manifest).ok())
                .unwrap_or_default();
            let prefix = repository_prefix(root).await;
            let scan = Scan {
                store,
                project_id,
                job: &job,
                manifest: &manifest,
                prefix: prefix.as_deref(),
            };
            scan.run(deadline).await
        }
    }
}

/// The root's job for its current head: kept when it already targets that head, otherwise
/// replaced by a new one listing the changes since the qualified head (`true`).
async fn job_for(
    store: &BlackboardStore,
    project_id: &str,
    root: &ObservedRoot<'_>,
    deadline: Instant,
) -> Result<(QualificationJob, bool), BlackboardStoreError> {
    let existing = store
        .qualification_jobs(project_id)
        .await?
        .into_iter()
        .find(|job| job.project_root == root.project_root);
    let job = match existing {
        Some(job) => job,
        // Nothing older is known than the previous observation (or this one).
        None => {
            store
                .establish_qualified_head(
                    project_id,
                    root.project_root,
                    root.previous_head.unwrap_or(root.head),
                )
                .await?
        }
    };
    let unchanged = match job.state {
        QualificationState::Complete => job.qualified_head == root.head,
        QualificationState::Scanning | QualificationState::Blocked => job.target_head == root.head,
    };
    if unchanged {
        return Ok((job, false));
    }
    let outcome = changed_paths(root, &job.qualified_head, deadline).await;
    store
        .start_qualification(
            project_id,
            root.project_root,
            &job.qualified_head,
            root.head,
            outcome,
        )
        .await
        .map(|job| (job, true))
}

/// The paths changed from `qualified` to the root's head, as a manifest, or why they cannot
/// be listed.
async fn changed_paths(
    root: &ObservedRoot<'_>,
    qualified: &str,
    deadline: Instant,
) -> Result<String, String> {
    let budget = GitObservationBudget::until(deadline);
    let (earlier, later) = (GitSha::new(qualified), GitSha::new(root.head));
    match commits_between(root.worktree_root, &earlier, &later, 1, &budget).await {
        GitCommitRange::Unchanged | GitCommitRange::Advanced { .. } => {}
        GitCommitRange::Discontinuous => {
            return Err(format!(
                "history moved from {} to {} without descending from it (a reset, rebase or branch switch)",
                short(qualified),
                short(root.head)
            ));
        }
        GitCommitRange::Unknown(failure) => {
            return Err(format!(
                "the commits from {} to {} could not be read ({failure:?})",
                short(qualified),
                short(root.head)
            ));
        }
    }
    match paths_changed_between(root.worktree_root, &earlier, &later, &budget).await {
        Ok(paths) if paths.len() <= MAX_CHANGED_PATHS => Ok(json!(paths).to_string()),
        Ok(paths) => Err(format!(
            "{} paths changed from {} to {}, more than the {MAX_CHANGED_PATHS} that are checked",
            paths.len(),
            short(qualified),
            short(root.head)
        )),
        Err(failure) => Err(format!(
            "the paths changed from {} to {} could not be listed ({failure:?})",
            short(qualified),
            short(root.head)
        )),
    }
}

/// The project root's path inside the Git worktree, with `/` separators (empty at the top),
/// or `None` when it cannot be established.
async fn repository_prefix(root: &ObservedRoot<'_>) -> Option<String> {
    let project_root = root.project_root.to_string();
    let worktree_root = root.worktree_root.as_path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let project = std::fs::canonicalize(Path::new(&project_root)).ok()?;
        let worktree = std::fs::canonicalize(&worktree_root).ok()?;
        let relative = project.strip_prefix(&worktree).ok()?;
        let parts = relative
            .components()
            .map(|component| component.as_os_str().to_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()?;
        Some(parts.join("/"))
    })
    .await
    .ok()
    .flatten()
}

struct Scan<'a> {
    store: &'a BlackboardStore,
    project_id: &'a str,
    job: &'a QualificationJob,
    manifest: &'a [String],
    /// The project root's path in the repository; `None` when it could not be mapped.
    prefix: Option<&'a str>,
}

impl Scan<'_> {
    /// Examines entries page by page from the job's cursor until the watermark, the per-turn
    /// budget or the deadline; completes the job when it reaches the watermark.
    async fn run(&self, deadline: Instant) -> Vec<String> {
        // Examination stops early enough to leave time for committing its progress.
        let work = deadline.checked_sub(PROGRESS_RESERVE).unwrap_or(deadline);
        let mut cursor = self.job.cursor;
        let mut examined = 0usize;
        let mut marked = 0usize;
        let mut finished = false;
        'pages: while examined < MAX_EXAMINED_PER_TURN {
            let page = self.store.qualification_page(
                self.project_id,
                cursor,
                self.job.watermark,
                PAGE_ENTRIES,
            );
            let page = match before(work, page).await {
                Some(Ok(page)) => page,
                Some(Err(_)) | None => break,
            };
            if page.is_empty() {
                // Nothing eligible is left below the watermark.
                cursor = self.job.watermark;
                finished = true;
                break;
            }
            for item in page {
                if examined == MAX_EXAMINED_PER_TURN {
                    break 'pages;
                }
                let sequence = item.sequence;
                match before(work, self.examine(item)).await {
                    Some(Ok(Examined::Marked)) => {
                        marked += 1;
                        cursor = sequence;
                        // Progress commits with each mark.
                        if !self.save(cursor, deadline).await {
                            break 'pages;
                        }
                    }
                    Some(Ok(Examined::Unaffected)) => cursor = sequence,
                    // A conflict that persists, an error or the deadline: this entry stays
                    // pending and the cursor stops before it.
                    Some(Ok(Examined::Pending) | Err(_)) | None => break 'pages,
                }
                examined += 1;
            }
        }
        if cursor > self.job.cursor {
            self.save(cursor, deadline).await;
        }
        if finished
            && !matches!(
                before(deadline, self.store.complete_qualification(self.job)).await,
                Some(Ok(true))
            )
        {
            finished = false;
        }
        let range = format!(
            "{}..{}",
            short(&self.job.qualified_head),
            short(&self.job.target_head)
        );
        let mut lines = Vec::new();
        if marked > 0 {
            lines.push(format!(
                "{marked} remembered conclusion(s) depend on files changed in {range} and now need a check against the current source."
            ));
        }
        if !finished {
            lines.push(format!(
                "Remembered code facts recorded before {} are still being checked against the files changed in {range}; treat them as unchecked.",
                short(&self.job.target_head)
            ));
        }
        lines
    }

    /// Records the cursor; false when the deadline passed or a newer job replaced this one.
    async fn save(&self, cursor: i64, deadline: Instant) -> bool {
        let progress = self.store.record_qualification_progress(self.job, cursor);
        matches!(before(deadline, progress).await, Some(Ok(true)))
    }

    /// Marks `item` when a changed path affects it, judging a concurrently changed entry once
    /// more from its new revision.
    async fn examine(&self, item: ScanItem) -> Result<Examined, BlackboardStoreError> {
        let mut entry = item.entry;
        for _ in 0..2 {
            let Some(reason) = self.affecting_change(&entry, &item.dependencies) else {
                return Ok(Examined::Unaffected);
            };
            let context = self
                .store
                .knowledge_context_at(self.project_id, &entry.id, entry.revision)
                .await?;
            if context
                .as_ref()
                .is_some_and(|context| context.validity != KnowledgeValidity::Current)
            {
                return Ok(Examined::Unaffected);
            }
            let category = context
                .as_ref()
                .map_or(KnowledgeCategory::Legacy, |context| context.category);
            let next = needs_check_context(context, &reason, &self.job.target_head);
            let record = ChangeRecord {
                operation: ChangeOperation::Invalidated,
                origin: ChangeOrigin::HostObserved,
                category,
                action_id: None,
                thread_id: None,
                turn_id: None,
                group_id: None,
                preview: entry.value.content.clone(),
            };
            match self
                .store
                .invalidate_entry(self.project_id, &entry.id, entry.revision, next, &record)
                .await
            {
                Ok(InvalidationOutcome::Invalidated) => return Ok(Examined::Marked),
                Ok(InvalidationOutcome::Unchanged) => return Ok(Examined::Unaffected),
                Err(BlackboardStoreError::RevisionConflict { .. }) => {
                    match self.store.get_entry(self.project_id, &entry.id).await? {
                        Some(reloaded) => entry = reloaded,
                        None => return Ok(Examined::Unaffected),
                    }
                }
                Err(BlackboardStoreError::EntryNotActive(_)) => return Ok(Examined::Unaffected),
                Err(error) => return Err(error),
            }
        }
        Ok(Examined::Pending)
    }

    /// Why a change of this job affects `entry`, if one does.
    fn affecting_change(
        &self,
        entry: &BlackboardEntry,
        dependencies: &[SourceDependency],
    ) -> Option<String> {
        if self.manifest.is_empty() || entry.value.kind == BlackboardKind::Instruction {
            return None;
        }
        let range = format!(
            "{}..{}",
            short(&self.job.qualified_head),
            short(&self.job.target_head)
        );
        if dependencies.is_empty() {
            return OBSERVATION_KINDS.contains(&entry.value.kind).then(|| {
                format!(
                    "it names no source file, and {} file(s) changed in {range}; verify it against the current source",
                    self.manifest.len()
                )
            });
        }
        for dependency in dependencies
            .iter()
            .filter(|dependency| dependency.project_root == self.job.project_root)
        {
            let Some(prefix) = self.prefix else {
                return Some(format!(
                    "its source {} could not be placed in the repository while files changed in {range}; verify it against the current source",
                    dependency.relative_path
                ));
            };
            let path = [prefix, dependency.relative_path.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/");
            let changed = self.manifest.iter().find(|changed| {
                **changed == path
                    || (dependency.directory
                        && (path.is_empty() || changed.starts_with(&format!("{path}/"))))
            });
            if let Some(changed) = changed {
                return Some(format!(
                    "{changed} changed in {range}; verify it against the current source"
                ));
            }
        }
        None
    }
}

/// What examining one entry did.
enum Examined {
    Marked,
    Unaffected,
    /// The entry kept changing; it stays pending for the next pass.
    Pending,
}

/// `context` (legacy when absent) marked as needing a check for `reason`, keeping other
/// recorded details.
fn needs_check_context(
    context: Option<KnowledgeContext>,
    reason: &str,
    target: &str,
) -> KnowledgeContext {
    let context = context.unwrap_or_else(|| {
        KnowledgeContext::new(KnowledgeCategory::Legacy, KnowledgeAuthority::LegacyUnknown)
    });
    let mut payload = match context.payload.as_deref() {
        None => json!({}),
        Some(text) => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(object @ serde_json::Value::Object(_)) => object,
            Ok(_) | Err(_) => json!({ "previous": text }),
        },
    };
    payload["invalidation"] = json!({ "reason": reason, "commit": target });
    KnowledgeContext {
        validity: KnowledgeValidity::NeedsCheck,
        payload: Some(payload.to_string()),
        ..context
    }
}

fn blocked_line(job: &QualificationJob) -> String {
    format!(
        "Remembered code facts recorded before {} could not be checked: {}. Treat them as unchecked and verify against the current source.",
        short(&job.target_head),
        job.reason
            .as_deref()
            .unwrap_or("the changes could not be listed")
    )
}

pub(crate) fn short(oid: &str) -> &str {
    &oid[..oid.len().min(8)]
}

#[cfg(test)]
#[path = "source_qualification_tests.rs"]
mod tests;
