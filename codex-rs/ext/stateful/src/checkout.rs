//! Changes to the project's checkout observed between Stateful turns.
//!
//! The host samples each project root (HEAD, dirty or clean) at the start and the end of
//! every Stateful turn. At a turn's start it compares the fresh sample with the newest stored
//! one: commits that advanced HEAD since then, a HEAD that moved to unrelated history, and
//! uncommitted changes now. Their origin is unknown (the user, another tool, or a concurrent
//! thread), and the report says so; attribution is left to commit messages and explicit
//! markings. Observed commits are kept as dated maintenance facts, not promoted.

use std::time::Duration;

use codex_git_utils::GitCommitRange;
use codex_git_utils::GitHeadObservation;
use codex_git_utils::GitObservationBudget;
use codex_git_utils::GitObservationFailure;
use codex_git_utils::GitRepositoryObservation;
use codex_git_utils::GitWorktreeObservation;
use codex_git_utils::changed_paths;
use codex_git_utils::commits_between;
use codex_git_utils::observe_repository;
use codex_git_utils::staged_changes;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RepositoryDirtyCoverage;
use codex_project_intelligence::RepositoryHead;
use codex_project_intelligence::RepositoryObservation;
use codex_project_intelligence::RepositoryObservationId;
use codex_project_intelligence::RepositoryRootObservation;
use codex_project_intelligence::RepositoryRootsCoverage;
use codex_project_intelligence::RepositoryUnknownReason;
use codex_project_intelligence::RepositoryWorktree;
use codex_project_intelligence::RootPromotion;
use codex_protocol::protocol::GitSha;
use codex_utils_absolute_path::AbsolutePathBuf;
use sha2::Digest;
use sha2::Sha256;

use crate::services::ProjectIntelligenceServices;

/// Roots sampled per observation.
const MAX_ROOTS: usize = 3;
/// Commits and dirty paths listed per root.
const MAX_LISTED: usize = 10;
/// Longest report rendered for one turn.
pub(crate) const MAX_REPORT_BYTES: usize = 1_536;
const OBSERVATION_BUDGET: Duration = Duration::from_secs(2);
const COMMIT_FACT_CONFIDENCE_BASIS_POINTS: u16 = 10_000;

/// What the turn's start found changed since the last observation, for this turn's packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckoutReport(pub(crate) String);

/// The longest a turn's start or end may spend on checkout observation, storage included.
const LIFECYCLE_BUDGET: Duration = Duration::from_secs(4);

/// Samples the project's roots, compares them with the newest stored observation, stores
/// observed commits as facts and the new sample as the latest observation. The new sample
/// becomes the baseline only when the comparison completed; otherwise the next turn retries
/// it. Bounded in time; failures only lose the report.
pub(crate) async fn observe_turn_start(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    roots: &[String],
    turn_id: &str,
) -> Option<CheckoutReport> {
    match tokio::time::timeout(
        LIFECYCLE_BUDGET,
        observe_turn_start_unbounded(services, project_id, roots, turn_id),
    )
    .await
    {
        Ok(report) => report,
        Err(_) => {
            tracing::warn!(%project_id, "checkout observation at turn start timed out");
            services.set_checkout_hold(project_id, /*held*/ true);
            None
        }
    }
}

async fn observe_turn_start_unbounded(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    roots: &[String],
    turn_id: &str,
) -> Option<CheckoutReport> {
    // Serialized with every other start and end of the project, so a hold set by one
    // thread is seen before another publishes a baseline.
    let Ok(_permit) = services.checkout_lock(project_id).acquire_owned().await else {
        return None;
    };
    // Held from the moment this start owns the project until its comparison completes: a
    // timeout drops this future (and the permit) at any await, and a turn end queued on
    // the permit must then still see the hold rather than publish over unprocessed changes.
    services.set_checkout_hold(project_id, /*held*/ true);
    let budget = GitObservationBudget::until(tokio::time::Instant::now() + OBSERVATION_BUDGET);
    let Some((current, samples)) = sample(project_id, roots, &budget).await else {
        return None;
    };
    let store = match services.repository_observations().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open repository observations");
            return None;
        }
    };
    let previous = match store.latest(project_id).await {
        Ok(previous) => previous,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to read the latest repository observation");
            return None;
        }
    };
    let (report, compared) = match previous {
        Some(previous) => {
            compare(services, project_id, turn_id, &previous, &samples, &budget).await
        }
        None => (None, true),
    };
    // An unknown HEAD is not a baseline: the last known one stays until a sample succeeds.
    let complete = compared && samples.iter().all(known_head);
    if complete && let Err(error) = store.record(current).await {
        tracing::warn!(%project_id, %error, "failed to record a repository observation");
    }
    services.set_checkout_hold(project_id, !complete);
    report
}

/// Samples the project's roots at the end of a turn, so the next turn compares against the
/// checkout as this turn left it. Bounded in time.
pub(crate) async fn observe_turn_end(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    roots: &[String],
) {
    let observe = async {
        let Ok(_permit) = services.checkout_lock(project_id).acquire_owned().await else {
            return;
        };
        // A turn of any thread that could not process the changes holds the baseline.
        if services.checkout_held(project_id) {
            return;
        }
        let budget = GitObservationBudget::until(tokio::time::Instant::now() + OBSERVATION_BUDGET);
        let Some((current, samples)) = sample(project_id, roots, &budget).await else {
            return;
        };
        if !samples.iter().all(known_head) {
            return;
        }
        match services.repository_observations().await {
            Ok(store) => {
                if let Err(error) = store.record(current).await {
                    tracing::warn!(%project_id, %error, "failed to record a repository observation");
                }
            }
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to open repository observations");
            }
        }
    };
    if tokio::time::timeout(LIFECYCLE_BUDGET, observe)
        .await
        .is_err()
    {
        tracing::warn!(%project_id, "checkout observation at turn end timed out");
    }
}

fn known_head(sample: &RootSample) -> bool {
    !matches!(sample.observation.head, GitHeadObservation::Unknown(_))
}

/// One root's sample: its configured spelling, path and Git observation.
struct RootSample {
    project_root: String,
    path: AbsolutePathBuf,
    observation: GitRepositoryObservation,
    /// Fingerprint of the uncommitted changes (paths, sizes, modification times) when the
    /// checkout is dirty and every changed path was listed.
    dirty_digest: Option<String>,
}

/// Paths listed when fingerprinting uncommitted changes.
const MAX_FINGERPRINT_PATHS: usize = 200;

/// Fingerprint of the uncommitted changes: each changed path's index and worktree status,
/// size and modification time. Paths are relative to the Git worktree root.
async fn dirty_digest(
    path: &AbsolutePathBuf,
    worktree_root: Option<&AbsolutePathBuf>,
    budget: &GitObservationBudget,
) -> Option<String> {
    let worktree_root = worktree_root?.as_path().to_path_buf();
    let listed = changed_paths(path, MAX_FINGERPRINT_PATHS, budget)
        .await
        .ok()?;
    if listed.omitted {
        return None;
    }
    // Staged blob IDs and modes catch staging changes the status letters do not show.
    let staged = staged_changes(path, budget).await.ok()?;
    let mut paths = listed.paths;
    paths.sort_by(|left, right| left.path.cmp(&right.path));
    // File metadata is read off the async task, so a stalled filesystem cannot hold the
    // turn past its lifecycle bound.
    tokio::task::spawn_blocking(move || {
        let mut parts = vec![format!("staged|{staged}")];
        for changed in &paths {
            let relative = &changed.path;
            let status = &changed.status;
            let metadata = std::fs::metadata(worktree_root.join(relative)).ok();
            let modified = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |elapsed| elapsed.as_nanos());
            let size = metadata.map_or(0, |metadata| metadata.len());
            parts.push(format!("{status}|{relative}|{size}|{modified}"));
        }
        digest(&parts.iter().map(String::as_str).collect::<Vec<_>>())
    })
    .await
    .ok()
}

async fn sample(
    project_id: &str,
    roots: &[String],
    budget: &GitObservationBudget,
) -> Option<(RepositoryObservation, Vec<RootSample>)> {
    if roots.is_empty() {
        return None;
    }
    let started_at_ms = now_ms();
    let mut samples = Vec::new();
    for root in roots.iter().take(MAX_ROOTS) {
        let Ok(path) = AbsolutePathBuf::from_absolute_path(root) else {
            continue;
        };
        let observation = observe_repository(&path, budget).await;
        let dirty_digest = match observation.worktree {
            GitWorktreeObservation::Dirty => {
                dirty_digest(&path, observation.worktree_root.as_ref(), budget).await
            }
            GitWorktreeObservation::Clean | GitWorktreeObservation::Unknown(_) => None,
        };
        samples.push(RootSample {
            project_root: root.clone(),
            path,
            observation,
            dirty_digest,
        });
    }
    let completed_at_ms = now_ms().max(started_at_ms);
    let mut sorted = roots.to_vec();
    sorted.sort();
    sorted.dedup();
    let omitted = roots.len().saturating_sub(MAX_ROOTS);
    let id = RepositoryObservationId::parse(format!(
        "stateful-observation-{}",
        digest(&[
            project_id,
            &completed_at_ms.to_string(),
            &format!("{sorted:?}")
        ])
    ))
    .ok()?;
    let observation = RepositoryObservation {
        id,
        project_id: project_id.to_string(),
        started_at_ms,
        completed_at_ms,
        roots_digest: digest(&sorted.iter().map(String::as_str).collect::<Vec<_>>()),
        roots_coverage: if omitted == 0 && samples.len() == roots.len() {
            RepositoryRootsCoverage::Complete
        } else {
            RepositoryRootsCoverage::Unknown
        },
        omitted_root_count: u32::try_from(omitted).unwrap_or(u32::MAX),
        roots: samples
            .iter()
            .map(|sample| RepositoryRootObservation {
                project_root: sample.project_root.clone(),
                git_worktree_root: sample
                    .observation
                    .worktree_root
                    .as_ref()
                    .map(|root| root.display().to_string()),
                head: match &sample.observation.head {
                    GitHeadObservation::Commit { oid, head_ref } => RepositoryHead::Commit {
                        oid: oid.0.clone(),
                        head_ref: head_ref.clone(),
                    },
                    GitHeadObservation::Unborn { head_ref } => RepositoryHead::Unborn {
                        head_ref: Some(head_ref.clone()),
                    },
                    GitHeadObservation::Unknown(failure) => RepositoryHead::Unknown {
                        reason: unknown_reason(*failure),
                    },
                },
                worktree: match &sample.observation.worktree {
                    GitWorktreeObservation::Clean => RepositoryWorktree::Clean,
                    GitWorktreeObservation::Dirty => RepositoryWorktree::Dirty {
                        coverage: match &sample.dirty_digest {
                            Some(digest) => RepositoryDirtyCoverage::Complete {
                                digest: digest.clone(),
                            },
                            None => RepositoryDirtyCoverage::Unknown {
                                reason: RepositoryUnknownReason::DirtyFingerprintNotCollected,
                            },
                        },
                    },
                    GitWorktreeObservation::Unknown(failure) => RepositoryWorktree::Unknown {
                        reason: unknown_reason(*failure),
                    },
                },
            })
            .collect(),
    };
    Some((observation, samples))
}

/// Compares the fresh samples with the previous observation. Returns the report and whether
/// the comparison completed (every history read and fact write succeeded).
async fn compare(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    turn_id: &str,
    previous: &RepositoryObservation,
    samples: &[RootSample],
    budget: &GitObservationBudget,
) -> (Option<CheckoutReport>, bool) {
    let mut complete = true;
    let mut lines = Vec::new();
    for sample in samples {
        let Some(before) = previous
            .roots
            .iter()
            .find(|root| root.project_root == sample.project_root)
        else {
            continue;
        };
        let mut root_lines = Vec::new();
        match (&before.head, &sample.observation.head) {
            (
                RepositoryHead::Commit { oid: old, .. },
                GitHeadObservation::Commit { oid: new, .. },
            ) => {
                match commits_between(&sample.path, &GitSha::new(old), new, MAX_LISTED, budget).await
                {
                    GitCommitRange::Unchanged => {}
                    GitCommitRange::Advanced { commits, omitted } => {
                        root_lines.push(format!(
                            "HEAD advanced by {} commit(s){}:",
                            commits.len(),
                            if omitted { " or more" } else { "" }
                        ));
                        for commit in &commits {
                            let short = &commit.oid[..commit.oid.len().min(8)];
                            let date = date_only(commit.committed_at.saturating_mul(1000));
                            let body = if commit.body.is_empty() {
                                String::new()
                            } else {
                                format!(" {}", single_line(&commit.body))
                            };
                            root_lines.push(format!(
                                "- {short} ({date}) {}{body}",
                                single_line(&commit.subject)
                            ));
                            complete &= store_commit_fact(
                                services,
                                project_id,
                                turn_id,
                                previous,
                                &sample.project_root,
                                &commit.oid,
                                &format!(
                                    "Commit {} (committed {date}) in {}, observed after {} between Stateful turns (origin unknown; full message: git show {}): {}{body}",
                                    commit.oid,
                                    sample.project_root,
                                    crate::continuity::format_time(previous.completed_at_ms),
                                    commit.oid,
                                    single_line(&commit.subject)
                                ),
                            )
                            .await;
                        }
                        if omitted {
                            root_lines.push(format!(
                                "- more: git log {}..{}",
                                &old[..old.len().min(8)],
                                &new.0[..new.0.len().min(8)]
                            ));
                        }
                    }
                    GitCommitRange::Discontinuous => root_lines.push(format!(
                        "HEAD moved from {} to unrelated history {} (rebase, reset or branch switch); check git log before relying on remembered file state.",
                        &old[..old.len().min(8)],
                        &new.0[..new.0.len().min(8)]
                    )),
                    GitCommitRange::Unknown(_) => {
                        complete = false;
                        root_lines.push("HEAD changed; the commits could not be read.".to_string());
                    }
                }
            }
            (RepositoryHead::Unborn { .. }, GitHeadObservation::Commit { oid, .. }) => {
                root_lines.push(format!(
                    "The first commit(s) were made (HEAD now {}); run git log.",
                    &oid.0[..oid.0.len().min(8)]
                ));
            }
            (RepositoryHead::Commit { .. }, GitHeadObservation::Unborn { .. }) => {
                root_lines.push("HEAD now names a branch with no commits.".to_string());
            }
            (RepositoryHead::Unborn { .. }, GitHeadObservation::Unborn { .. }) => {}
            (RepositoryHead::Unknown { .. }, GitHeadObservation::Unknown(_)) => {}
            (_, GitHeadObservation::Unknown(_)) | (RepositoryHead::Unknown { .. }, _) => {
                root_lines.push("Whether HEAD changed is unknown.".to_string());
            }
        }
        match (&before.worktree, &sample.observation.worktree) {
            (_, GitWorktreeObservation::Dirty) => {
                // The same fingerprint means the uncommitted changes did not change.
                let unchanged = matches!(
                    (&before.worktree, &sample.dirty_digest),
                    (
                        RepositoryWorktree::Dirty {
                            coverage: RepositoryDirtyCoverage::Complete { digest: old },
                        },
                        Some(new),
                    ) if old == new
                );
                if !unchanged {
                    match changed_paths(&sample.path, MAX_LISTED, budget).await {
                        Ok(paths) => root_lines.push(format!(
                            "Uncommitted changes now: {}{}",
                            paths
                                .paths
                                .iter()
                                .map(|changed| format!(
                                    "{} ({})",
                                    single_line(&changed.path),
                                    changed.status.trim()
                                ))
                                .collect::<Vec<_>>()
                                .join(", "),
                            if paths.omitted { ", ..." } else { "" }
                        )),
                        Err(_) => {
                            root_lines.push("Uncommitted changes now (paths unknown).".to_string())
                        }
                    }
                }
            }
            (RepositoryWorktree::Dirty { .. }, GitWorktreeObservation::Clean) => {
                root_lines.push(
                    "The uncommitted changes seen before are gone (committed, reverted or removed); the checkout is clean."
                        .to_string(),
                );
            }
            (RepositoryWorktree::Unknown { .. }, GitWorktreeObservation::Clean)
            | (RepositoryWorktree::Clean, GitWorktreeObservation::Clean)
            | (RepositoryWorktree::Unknown { .. }, GitWorktreeObservation::Unknown(_)) => {}
            (_, GitWorktreeObservation::Unknown(_)) => {
                root_lines.push("Whether there are uncommitted changes is unknown.".to_string());
            }
        }
        if !root_lines.is_empty() {
            lines.push(format!("{}:", single_line(&sample.project_root)));
            lines.extend(root_lines);
        }
    }
    if lines.is_empty() {
        return (None, complete);
    }
    (
        Some(CheckoutReport(render_report(
            previous.completed_at_ms,
            lines,
        ))),
        complete,
    )
}

/// The report text, bounded after escaping (notice included) so the fragment never exceeds
/// `MAX_REPORT_BYTES`.
fn render_report(observed_at_ms: i64, lines: Vec<String>) -> String {
    // Bounded after escaping, notice included, so the fragment never exceeds the cap.
    let observed_at = crate::continuity::format_time(observed_at_ms);
    let mut report = crate::continuity::escape(&format!(
        "Checkout changes since the last observation ({observed_at}, at a Stateful turn in this project). Their origin is unknown: the user, another tool, or a concurrent thread. Keep existing work; credit the user only where commit messages or explicit markings support it, and keep lines the user marked as theirs when editing nearby."
    ));
    let notice = "\n... more changes: run git log and git status.";
    for line in lines {
        let next = crate::continuity::escape(&format!("\n{line}"));
        if report.len() + next.len() + notice.len() > MAX_REPORT_BYTES {
            let room = MAX_REPORT_BYTES.saturating_sub(report.len() + notice.len());
            if room > 40 {
                let mut end = room;
                while !next.is_char_boundary(end) {
                    end -= 1;
                }
                // Never cut inside an escape sequence such as \u003c.
                let cut = &next[..end];
                let cut = match cut.rfind('\\') {
                    Some(position) if cut.len() - position < 6 => &cut[..position],
                    Some(_) | None => cut,
                };
                report.push_str(cut);
            }
            report.push_str(notice);
            break;
        }
        report.push_str(&next);
    }
    report
}

fn date_only(ms: i64) -> String {
    let time = crate::continuity::format_time(ms);
    time.split(' ').next().unwrap_or(&time).to_string()
}

/// Stores one observed commit as a dated maintenance fact; returns whether it is stored.
async fn store_commit_fact(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    turn_id: &str,
    previous: &RepositoryObservation,
    project_root: &str,
    oid: &str,
    content: &str,
) -> bool {
    let observation_id = &previous.id;
    let Ok(id) = BlackboardEntryId::parse(format!(
        "stateful-commit-{}",
        digest(&[project_id, project_root, oid])
    )) else {
        return false;
    };
    let result = async {
        let store = services
            .blackboard()
            .await
            .map_err(|error| error.to_string())?;
        if store
            .get_entry(project_id, &id)
            .await
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        let node_id = services.project_node_id(project_id).await?;
        let mut content = content.to_string();
        if content.len() > 1_000 {
            let mut end = 1_000;
            while !content.is_char_boundary(end) {
                end -= 1;
            }
            content.truncate(end);
            content.push_str("...");
        }
        store
            .create_entry(
                id,
                NewBlackboardEntry {
                    project_id: project_id.to_string(),
                    node_id,
                    kind: BlackboardKind::Fact,
                    content,
                    structured_value: None,
                    confidence: ConfidenceScore::from_basis_points(
                        COMMIT_FACT_CONFIDENCE_BASIS_POINTS,
                    )
                    .map_err(|error| error.to_string())?,
                    verification: BlackboardVerification::Unverified,
                    importance: BlackboardImportance::Normal,
                    root_promotion: RootPromotion::NotPromoted,
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    provenance: BlackboardProvenance {
                        kind: BlackboardProvenanceKind::Maintenance,
                        source_id: format!("checkout-observation:{observation_id}/{turn_id}"),
                    },
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    .await;
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to store an observed commit");
            false
        }
    }
}

fn unknown_reason(failure: GitObservationFailure) -> RepositoryUnknownReason {
    match failure {
        GitObservationFailure::MissingRoot => RepositoryUnknownReason::MissingRoot,
        GitObservationFailure::GitUnavailable => RepositoryUnknownReason::GitUnavailable,
        GitObservationFailure::Spawn(_)
        | GitObservationFailure::Containment(_)
        | GitObservationFailure::Io(_) => RepositoryUnknownReason::InaccessibleRoot,
        GitObservationFailure::Timeout => RepositoryUnknownReason::Timeout,
        GitObservationFailure::OutputLimit => RepositoryUnknownReason::OutputLimit,
        GitObservationFailure::CommandFailed { .. } => RepositoryUnknownReason::GitCommandFailed,
        GitObservationFailure::InvalidOutput => RepositoryUnknownReason::InvalidGitOutput,
        GitObservationFailure::UnsupportedPath => RepositoryUnknownReason::UnsupportedPath,
        GitObservationFailure::UnstableSample => RepositoryUnknownReason::UnstableSample,
    }
}

fn digest(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
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
#[path = "checkout_tests.rs"]
mod tests;

pub(crate) const WORLD_STATE_ID: &str = "stateful_checkout_changes";
pub(crate) const START_MARKER: &str = "<stateful_checkout_changes>";
pub(crate) const END_MARKER: &str = "</stateful_checkout_changes>";
/// Checkout reports one context window may hold; past it a turn gets one short line.
pub(crate) const MAX_WINDOW_REPORT_BYTES: usize = 3 * 1024;
const OVER_BUDGET_REPORT: &str = "Further checkout changes were observed since the last Stateful turn; run git log and git status.";

/// The checkout report planned for one sampling step and the report bytes the window holds.
pub(crate) struct CheckoutReportPlan {
    turn_id: String,
    body: Option<String>,
    /// Which form this turn's report took: "full", "fallback" or "none".
    shown: String,
    pub(crate) window_bytes: usize,
}

impl CheckoutReportPlan {
    /// Plans this turn's report against the previous snapshot of the section. A report
    /// renders once per turn (again if a new window lost it) and counts against the window.
    pub(crate) fn new(
        previous: Option<&serde_json::Value>,
        turn_id: &str,
        report: Option<&CheckoutReport>,
    ) -> Self {
        let field = |name: &str| previous.and_then(|previous| previous.get(name));
        let same_turn = field("turnId").and_then(serde_json::Value::as_str) == Some(turn_id);
        let previous_bytes = field("windowBytes")
            .and_then(serde_json::Value::as_u64)
            .and_then(|bytes| usize::try_from(bytes).ok())
            .unwrap_or(0);
        let fragment_bytes = |body: &str| START_MARKER.len() + body.len() + END_MARKER.len();
        let fallback_bytes = fragment_bytes(OVER_BUDGET_REPORT);
        // A later step of the same turn shows the form admitted at its first step, uncharged.
        let shown = if same_turn {
            field("shown")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none")
                .to_string()
        } else {
            match report {
                Some(report)
                    if previous_bytes
                        .saturating_add(fragment_bytes(&report.0))
                        .saturating_add(fallback_bytes)
                        <= MAX_WINDOW_REPORT_BYTES =>
                {
                    "full".to_string()
                }
                Some(_)
                    if previous_bytes.saturating_add(fallback_bytes) <= MAX_WINDOW_REPORT_BYTES =>
                {
                    "fallback".to_string()
                }
                Some(_) | None => "none".to_string(),
            }
        };
        let body = match (shown.as_str(), report) {
            ("full", Some(report)) => Some(report.0.clone()),
            ("fallback", Some(_)) => Some(OVER_BUDGET_REPORT.to_string()),
            _ => None,
        };
        let added = match (&body, same_turn) {
            (Some(body), false) => fragment_bytes(body),
            (Some(_), true) | (None, _) => 0,
        };
        Self {
            turn_id: turn_id.to_string(),
            body,
            shown,
            window_bytes: previous_bytes.saturating_add(added),
        }
    }

    pub(crate) fn section(self) -> codex_extension_api::WorldStateSectionContribution {
        let Self {
            turn_id,
            body,
            shown,
            window_bytes,
        } = self;
        codex_extension_api::WorldStateSectionContribution::new(
            WORLD_STATE_ID,
            serde_json::json!({ "turnId": turn_id, "shown": shown, "windowBytes": window_bytes }),
            move |previous| {
                if let codex_extension_api::PreviousWorldStateSection::Known(previous) = previous
                    && previous.get("turnId").and_then(serde_json::Value::as_str)
                        == Some(turn_id.as_str())
                {
                    return None;
                }
                body.as_ref().map(|body| {
                    codex_extension_api::RenderedWorldStateFragment::new(
                        "developer",
                        (START_MARKER, END_MARKER),
                        body,
                    )
                })
            },
        )
        .with_retained_fragment_matcher(|role, text| {
            role == "developer"
                && text.trim_start().starts_with(START_MARKER)
                && text.trim_end().ends_with(END_MARKER)
        })
    }
}
