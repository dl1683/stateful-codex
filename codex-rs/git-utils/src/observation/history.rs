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
const FIELD_SEPARATOR: char = '\u{1f}';
const RECORD_SEPARATOR: char = '\u{1e}';

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

/// Paths with uncommitted changes (tracked or untracked), at most the requested limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitWorktreePaths {
    pub paths: Vec<String>,
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
    let format = format!(
        "--format=%H{FIELD_SEPARATOR}%ct{FIELD_SEPARATOR}%s{FIELD_SEPARATOR}%b{RECORD_SEPARATOR}"
    );
    let range = format!("{}..{}", earlier.0, later.0);
    let log = match run(
        cwd,
        &[
            "log",
            "--no-color",
            &count,
            &format,
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
    for record in text.split(RECORD_SEPARATOR) {
        let record = record.trim_start_matches(['\n', '\r']);
        if record.is_empty() {
            continue;
        }
        let mut fields = record.splitn(4, FIELD_SEPARATOR);
        let (Some(oid), Some(time), Some(subject)) = (fields.next(), fields.next(), fields.next())
        else {
            return GitCommitRange::Unknown(GitObservationFailure::InvalidOutput);
        };
        let Ok(committed_at) = time.trim().parse::<i64>() else {
            return GitCommitRange::Unknown(GitObservationFailure::InvalidOutput);
        };
        commits.push(GitCommitSummary {
            oid: oid.trim().to_string(),
            committed_at,
            subject: bounded(subject.trim(), MAX_SUBJECT_BYTES),
            body: bounded(fields.next().unwrap_or_default().trim(), MAX_BODY_BYTES),
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
        .filter(|entry| entry.len() > 3)
        .map(|entry| entry[3..].to_string())
        .collect::<Vec<_>>();
    let omitted = paths.len() > limit;
    paths.truncate(limit);
    Ok(GitWorktreePaths { paths, omitted })
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
