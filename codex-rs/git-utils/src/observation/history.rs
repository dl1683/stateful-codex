//! Bounded reads of what changed in a checkout between two observations: the commits that
//! advanced HEAD, and the paths with uncommitted changes now.
//!
//! These share an observation's [`GitObservationBudget`], so a whole comparison runs under
//! one deadline and output allowance. HEAD moving to a commit that does not descend from the
//! earlier one (a rebase, reset or branch switch) is reported as discontinuous rather than
//! as a list of new commits.

use std::path::Path;

use codex_protocol::protocol::GitSha;
use codex_utils_absolute_path::AbsolutePathBuf;

use super::GitObservationBudget;
use super::GitObservationFailure;
use super::command_error;
use super::command_failed;
use super::probe::hardened_git_command;
use crate::git_process::GitCommandOutputCap;
use crate::git_process::run_git_command_with_budget;

/// Longest body excerpt kept per commit.
const MAX_BODY_BYTES: usize = 400;
/// Longest subject kept per commit.
const MAX_SUBJECT_BYTES: usize = 240;

/// One commit that advanced HEAD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCommitSummary {
    pub oid: String,
    /// Committer time, Unix seconds.
    pub committed_at: i64,
    pub subject: String,
    /// The start of the message body, at most 400 bytes; empty when there is none.
    pub body: String,
}

/// How HEAD moved between two observed commits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GitCommitRange {
    Unchanged,
    /// The earlier commit is an ancestor of the later one. `commits` are the newest
    /// first, at most the requested limit; `omitted` is set when more exist.
    Advanced {
        commits: Vec<GitCommitSummary>,
        omitted: bool,
    },
    /// The later commit does not descend from the earlier one.
    Discontinuous,
    Unknown(GitObservationFailure),
}

/// One path with uncommitted changes and its two-letter porcelain status (index, worktree).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitChangedPath {
    pub status: String,
    /// Relative to the Git worktree root.
    pub path: String,
}

/// Paths with uncommitted changes (tracked, staged or untracked), at most the requested limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitWorktreePaths {
    pub paths: Vec<GitChangedPath>,
    pub omitted: bool,
}

/// The commits that took HEAD from `earlier` to `later` in the checkout at `root`.
pub async fn commits_between(
    root: &AbsolutePathBuf,
    earlier: &GitSha,
    later: &GitSha,
    limit: usize,
    budget: &GitObservationBudget,
) -> GitCommitRange {
    if earlier.0 == later.0 {
        return GitCommitRange::Unchanged;
    }
    let cwd = root.as_path();
    let ancestry = match run(
        cwd,
        &[
            "merge-base",
            "--is-ancestor",
            "--end-of-options",
            &earlier.0,
            &later.0,
        ],
        budget,
        GitCommandOutputCap::Metadata,
    )
    .await
    {
        Ok(output) => output,
        Err(failure) => return GitCommitRange::Unknown(failure),
    };
    match ancestry.status.code() {
        Some(0) => {}
        Some(1) => return GitCommitRange::Discontinuous,
        _ => return GitCommitRange::Unknown(command_failed(&ancestry)),
    }
    let count = format!("--max-count={}", limit.saturating_add(1));
    let range = format!("{}..{}", earlier.0, later.0);
    // Records end with NUL, which no commit message can contain; within a record the first
    // two lines are the OID and committer time and the rest is the raw message.
    let log = match run(
        cwd,
        &[
            "-c",
            "i18n.logOutputEncoding=UTF-8",
            "-c",
            "log.showSignature=false",
            "log",
            "-z",
            "--no-color",
            "--encoding=UTF-8",
            &count,
            "--format=%H%n%ct%n%B",
            "--end-of-options",
            &range,
        ],
        budget,
        GitCommandOutputCap::SharedAllowance,
    )
    .await
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => return GitCommitRange::Unknown(command_failed(&output)),
        Err(failure) => return GitCommitRange::Unknown(failure),
    };
    let text = String::from_utf8_lossy(&log.stdout);
    let mut commits = Vec::new();
    for record in text.split('\0') {
        let record = record.trim_start_matches(['\n', '\r']);
        if record.is_empty() {
            continue;
        }
        let mut lines = record.splitn(3, '\n');
        let (Some(oid), Some(time)) = (lines.next(), lines.next()) else {
            return GitCommitRange::Unknown(GitObservationFailure::InvalidOutput);
        };
        let oid = oid.trim();
        let Ok(committed_at) = time.trim().parse::<i64>() else {
            return GitCommitRange::Unknown(GitObservationFailure::InvalidOutput);
        };
        if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return GitCommitRange::Unknown(GitObservationFailure::InvalidOutput);
        }
        let message = lines.next().unwrap_or_default();
        let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
        commits.push(GitCommitSummary {
            oid: oid.to_string(),
            committed_at,
            subject: bounded(subject.trim(), MAX_SUBJECT_BYTES),
            body: bounded(body.trim(), MAX_BODY_BYTES),
        });
    }
    let omitted = commits.len() > limit;
    commits.truncate(limit);
    GitCommitRange::Advanced { commits, omitted }
}

/// The paths with uncommitted changes in the checkout at `root`.
pub async fn changed_paths(
    root: &AbsolutePathBuf,
    limit: usize,
    budget: &GitObservationBudget,
) -> Result<GitWorktreePaths, GitObservationFailure> {
    let output = run(
        root.as_path(),
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
            "--no-renames",
        ],
        budget,
        GitCommandOutputCap::SharedAllowance,
    )
    .await?;
    if !output.status.success() {
        return Err(command_failed(&output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut paths = text
        .split('\0')
        .filter(|entry| entry.len() > 3 && entry.is_char_boundary(2) && entry.is_char_boundary(3))
        .map(|entry| GitChangedPath {
            status: entry[..2].to_string(),
            path: entry[3..].to_string(),
        })
        .collect::<Vec<_>>();
    let omitted = paths.len() > limit;
    paths.truncate(limit);
    Ok(GitWorktreePaths { paths, omitted })
}

/// The staged changes as Git's raw diff of the index against HEAD (modes and blob IDs per
/// path), for fingerprinting what is staged even when the status letters do not change.
pub async fn staged_changes(
    root: &AbsolutePathBuf,
    budget: &GitObservationBudget,
) -> Result<String, GitObservationFailure> {
    let output = run(
        root.as_path(),
        &[
            "diff",
            "--cached",
            // Repository-wide even when diff.relative is configured, like porcelain status.
            "--no-relative",
            "--raw",
            "-z",
            "--no-renames",
            "--no-color",
            "--ignore-submodules=none",
        ],
        budget,
        GitCommandOutputCap::SharedAllowance,
    )
    .await?;
    if !output.status.success() {
        return Err(command_failed(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The patch commit `oid` made against its first parent (or the empty tree for a root
/// commit): unified diff with three context lines, no rename detection, no external diff or
/// text conversion. Bounded by the budget's shared output allowance.
pub async fn commit_patch(
    root: &AbsolutePathBuf,
    oid: &GitSha,
    budget: &GitObservationBudget,
) -> Result<String, GitObservationFailure> {
    let output = run(
        root.as_path(),
        &[
            "-c",
            "core.quotePath=false",
            "diff-tree",
            "--no-commit-id",
            "-p",
            "--unified=3",
            "--no-color",
            "--no-renames",
            "--no-ext-diff",
            "--root",
            "-m",
            "--first-parent",
            "--end-of-options",
            &oid.0,
        ],
        budget,
        GitCommandOutputCap::SharedAllowance,
    )
    .await?;
    if !output.status.success() {
        return Err(command_failed(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn run(
    cwd: &Path,
    args: &[&str],
    budget: &GitObservationBudget,
    cap: GitCommandOutputCap,
) -> Result<std::process::Output, GitObservationFailure> {
    let mut command = hardened_git_command(cwd, args);
    run_git_command_with_budget(&mut command, &budget.commands, cap)
        .await
        .map_err(|error| command_error(error, cwd))
}

fn bounded(text: &str, maximum: usize) -> String {
    if text.len() <= maximum {
        return text.to_string();
    }
    let mut end = maximum;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
