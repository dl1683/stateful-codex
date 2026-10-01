//! Bounded observation of a repository's HEAD and working-tree state.
//!
//! Every Git command runs under one shared deadline and output allowance, in a
//! contained process tree, with repository-selected hooks and fsmonitor
//! helpers disabled. HEAD is sampled before and after `git status`; any
//! disagreement between the samples and the status branch headers makes both
//! components [`GitObservationFailure::UnstableSample`]. This detects endpoint
//! disagreement, not an atomic filesystem snapshot.
//!
//! [`GitWorktreeObservation::Clean`] means Git reported no tracked change or
//! untracked file under this policy. It says nothing about ignored files.

use std::io;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use codex_protocol::protocol::GitSha;
use codex_utils_absolute_path::AbsolutePathBuf;
use tokio::time::Instant;

use crate::git_process::GitCommandBudget;
use crate::git_process::GitCommandError;
use crate::git_process::GitSpawnError;

mod parse;
mod probe;

use parse::ParseFailure;
use parse::PorcelainStatus;
use parse::StatusBranchHead;
use parse::StatusBranchOid;
use parse::StatusWorktree;
use probe::BoundedGitRunner;
use probe::GitProbe;
use probe::GitProbeRunner;
use probe::HeadEndpoint;

/// The longest any budget may run, measured from its creation.
const MAX_OBSERVATION_DURATION: Duration = Duration::from_secs(2);
/// Cumulative stdout plus stderr shared by every command under one budget.
const OUTPUT_ALLOWANCE_BYTES: usize = 1024 * 1024;

/// A deadline and cumulative output allowance shared by every repository
/// observed against it. Neither is reset by retries or later observations.
#[derive(Debug)]
pub struct GitObservationBudget {
    commands: GitCommandBudget,
}

impl GitObservationBudget {
    /// Creates a budget ending at `deadline`, or two seconds from now if that
    /// is sooner, with a 1 MiB output allowance.
    pub fn until(deadline: Instant) -> Self {
        let deadline = deadline.min(Instant::now() + MAX_OBSERVATION_DURATION);
        Self {
            commands: GitCommandBudget::new(deadline, OUTPUT_ALLOWANCE_BYTES),
        }
    }
}

/// What Git reported about one directory, component by component.
#[derive(Clone, Debug, PartialEq)]
pub struct GitRepositoryObservation {
    /// The worktree containing the observed directory, when Git reported it.
    pub worktree_root: Option<AbsolutePathBuf>,
    pub head: GitHeadObservation,
    pub worktree: GitWorktreeObservation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GitHeadObservation {
    /// HEAD resolves to a commit; `head_ref` is `None` when it is detached.
    Commit {
        oid: GitSha,
        head_ref: Option<String>,
    },
    /// HEAD names a branch with no commit yet.
    Unborn {
        head_ref: String,
    },
    Unknown(GitObservationFailure),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GitWorktreeObservation {
    Clean,
    Dirty,
    Unknown(GitObservationFailure),
}

/// An I/O error reduced to comparable diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitIoFailure {
    pub kind: io::ErrorKind,
    pub raw_os_error: Option<i32>,
}

impl From<&io::Error> for GitIoFailure {
    fn from(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
        }
    }
}

/// Why a component of an observation could not be established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitObservationFailure {
    /// The observed directory does not exist or is not a directory.
    MissingRoot,
    /// The Git executable could not be found.
    GitUnavailable,
    /// Git could not be started. With required containment on Windows this
    /// includes failing to place it in its job, so it does not by itself mean
    /// Git is unavailable.
    Spawn(GitIoFailure),
    /// The process tree could not be contained for cleanup.
    Containment(GitIoFailure),
    /// Reading Git's output or exit status failed.
    Io(GitIoFailure),
    Timeout,
    OutputLimit,
    /// Git exited unsuccessfully without a more specific established cause.
    /// Stderr is deliberately not retained.
    CommandFailed {
        exit_code: Option<i32>,
    },
    /// Git succeeded but its output was malformed or inconsistent.
    InvalidOutput,
    /// Git reported a path this platform cannot represent.
    UnsupportedPath,
    /// HEAD differed between samples taken during the observation.
    UnstableSample,
}

impl From<ParseFailure> for GitObservationFailure {
    fn from(failure: ParseFailure) -> Self {
        match failure {
            ParseFailure::InvalidOutput => Self::InvalidOutput,
            ParseFailure::UnsupportedPath => Self::UnsupportedPath,
        }
    }
}

/// Observes the worktree containing `root` within `budget`.
///
/// Never retries: a failure or an unstable sample is reported as unknown.
pub async fn observe_repository(
    root: &AbsolutePathBuf,
    budget: &GitObservationBudget,
) -> GitRepositoryObservation {
    observe_with(&BoundedGitRunner, root.as_path(), &budget.commands).await
}

/// HEAD as established by one endpoint sample.
#[derive(Clone, Debug, PartialEq)]
enum HeadSample {
    Commit {
        oid: GitSha,
        head_ref: Option<String>,
    },
    /// The symbolic ref names a branch that does not resolve. Only a status
    /// header reporting the initial state confirms it as unborn.
    Unresolved { head_ref: String },
}

async fn observe_with(
    runner: &impl GitProbeRunner,
    root: &Path,
    budget: &GitCommandBudget,
) -> GitRepositoryObservation {
    let root_output = match successful_probe(runner, GitProbe::WorktreeRoot, root, budget).await {
        Ok(output) => output,
        Err(failure) => return unknown_observation(/*worktree_root*/ None, failure),
    };
    let worktree_root = match parse::parse_worktree_root(&root_output)
        .map_err(GitObservationFailure::from)
        .and_then(|path| {
            AbsolutePathBuf::from_absolute_path_checked(path)
                .map_err(|_| GitObservationFailure::UnsupportedPath)
        }) {
        Ok(worktree_root) => worktree_root,
        Err(failure) => return unknown_observation(/*worktree_root*/ None, failure),
    };
    let cwd = worktree_root.as_path();
    let format = match successful_probe(runner, GitProbe::ObjectFormat, cwd, budget)
        .await
        .and_then(|output| Ok(parse::parse_object_format(&output)?))
    {
        Ok(format) => format,
        Err(failure) => return unknown_observation(Some(worktree_root), failure),
    };
    let initial = match sample_head(runner, cwd, budget, format, HeadEndpoint::Initial).await {
        Ok(initial) => initial,
        Err(failure) => return unknown_observation(Some(worktree_root), failure),
    };
    let status = successful_probe(runner, GitProbe::Status, cwd, budget)
        .await
        .and_then(|output| Ok(parse::parse_porcelain_v2_status(&output, format)?));
    let last = sample_head(runner, cwd, budget, format, HeadEndpoint::Final).await;
    let (head, worktree) = reconcile(initial, status, last);
    GitRepositoryObservation {
        worktree_root: Some(worktree_root),
        head,
        worktree,
    }
}

fn unknown_observation(
    worktree_root: Option<AbsolutePathBuf>,
    failure: GitObservationFailure,
) -> GitRepositoryObservation {
    GitRepositoryObservation {
        worktree_root,
        head: GitHeadObservation::Unknown(failure),
        worktree: GitWorktreeObservation::Unknown(failure),
    }
}

/// Combines both HEAD samples and the status response. HEAD is established
/// only when the endpoints agree with each other and with the status headers.
fn reconcile(
    initial: HeadSample,
    status: Result<PorcelainStatus, GitObservationFailure>,
    last: Result<HeadSample, GitObservationFailure>,
) -> (GitHeadObservation, GitWorktreeObservation) {
    let last = match last {
        Ok(last) => last,
        // A failed final probe cannot establish stability; keep an
        // independent status failure rather than replacing it.
        Err(failure) => {
            let worktree = match status {
                Ok(_) => failure,
                Err(status_failure) => status_failure,
            };
            return (
                GitHeadObservation::Unknown(failure),
                GitWorktreeObservation::Unknown(worktree),
            );
        }
    };
    let status_agrees = match &status {
        Ok(status) => status_matches(status, &initial),
        Err(_) => true,
    };
    if initial != last || !status_agrees {
        let failure = GitObservationFailure::UnstableSample;
        return (
            GitHeadObservation::Unknown(failure),
            GitWorktreeObservation::Unknown(failure),
        );
    }
    match status {
        Ok(status) => {
            let head = match initial {
                HeadSample::Commit { oid, head_ref } => {
                    GitHeadObservation::Commit { oid, head_ref }
                }
                HeadSample::Unresolved { head_ref } => GitHeadObservation::Unborn { head_ref },
            };
            let worktree = match status.worktree {
                StatusWorktree::Clean => GitWorktreeObservation::Clean,
                StatusWorktree::Dirty => GitWorktreeObservation::Dirty,
            };
            (head, worktree)
        }
        Err(failure) => {
            let head = match initial {
                HeadSample::Commit { oid, head_ref } => {
                    GitHeadObservation::Commit { oid, head_ref }
                }
                HeadSample::Unresolved { .. } => GitHeadObservation::Unknown(failure),
            };
            (head, GitWorktreeObservation::Unknown(failure))
        }
    }
}

fn status_matches(status: &PorcelainStatus, sample: &HeadSample) -> bool {
    let head_ref = match (sample, &status.oid) {
        (HeadSample::Commit { oid, head_ref }, StatusBranchOid::Commit(status_oid)) => {
            if oid != status_oid {
                return false;
            }
            head_ref.as_deref()
        }
        (HeadSample::Unresolved { head_ref }, StatusBranchOid::Initial) => Some(head_ref.as_str()),
        (HeadSample::Commit { .. }, StatusBranchOid::Initial)
        | (HeadSample::Unresolved { .. }, StatusBranchOid::Commit(_)) => return false,
    };
    match (&status.head, head_ref) {
        (StatusBranchHead::Detached, None) => true,
        (StatusBranchHead::Branch(name), Some(head_ref)) => {
            head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref) == name
        }
        (StatusBranchHead::Detached, Some(_)) | (StatusBranchHead::Branch(_), None) => false,
    }
}

async fn sample_head(
    runner: &impl GitProbeRunner,
    cwd: &Path,
    budget: &GitCommandBudget,
    format: parse::GitObjectFormat,
    endpoint: HeadEndpoint,
) -> Result<HeadSample, GitObservationFailure> {
    let symbolic = run_probe(runner, GitProbe::SymbolicRef(endpoint), cwd, budget).await?;
    // `symbolic-ref -q` exits 1 silently only for a detached HEAD.
    let head_ref = match symbolic.status.code() {
        Some(0) => Some(parse::parse_symbolic_ref(&symbolic.stdout)?),
        Some(1) if symbolic.stdout.is_empty() => None,
        _ => return Err(command_failed(&symbolic)),
    };
    let commit = run_probe(runner, GitProbe::HeadCommit(endpoint), cwd, budget).await?;
    if commit.status.success() {
        let oid = parse::parse_oid_line(&commit.stdout, format)?;
        return Ok(HeadSample::Commit { oid, head_ref });
    }
    // Distinguish a branch that does not resolve from a HEAD that resolves
    // to something other than a commit.
    let raw = run_probe(runner, GitProbe::RawHead(endpoint), cwd, budget).await?;
    match (raw.status.code(), head_ref) {
        (Some(1), Some(head_ref)) if raw.stdout.is_empty() => {
            Ok(HeadSample::Unresolved { head_ref })
        }
        _ => Err(command_failed(&commit)),
    }
}

async fn successful_probe(
    runner: &impl GitProbeRunner,
    probe: GitProbe,
    cwd: &Path,
    budget: &GitCommandBudget,
) -> Result<Vec<u8>, GitObservationFailure> {
    let output = run_probe(runner, probe, cwd, budget).await?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(command_failed(&output))
    }
}

async fn run_probe(
    runner: &impl GitProbeRunner,
    probe: GitProbe,
    cwd: &Path,
    budget: &GitCommandBudget,
) -> Result<Output, GitObservationFailure> {
    runner
        .run(probe, cwd, budget)
        .await
        .map_err(|error| match error {
            GitCommandError::Spawn(GitSpawnError::Spawn(error)) => {
                // Starting a process in a missing directory fails like a
                // missing executable, so check the directory first. Only an
                // established absence counts as missing.
                match std::fs::metadata(cwd) {
                    Ok(metadata) if !metadata.is_dir() => GitObservationFailure::MissingRoot,
                    Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => {
                        GitObservationFailure::MissingRoot
                    }
                    Ok(_) if error.kind() == io::ErrorKind::NotFound => {
                        GitObservationFailure::GitUnavailable
                    }
                    Ok(_) | Err(_) => GitObservationFailure::Spawn((&error).into()),
                }
            }
            GitCommandError::Spawn(GitSpawnError::Containment(error)) => {
                GitObservationFailure::Containment((&error).into())
            }
            GitCommandError::Timeout => GitObservationFailure::Timeout,
            GitCommandError::OutputLimit => GitObservationFailure::OutputLimit,
            GitCommandError::Io(error) => GitObservationFailure::Io((&error).into()),
        })
}

fn command_failed(output: &Output) -> GitObservationFailure {
    GitObservationFailure::CommandFailed {
        exit_code: output.status.code(),
    }
}

#[cfg(test)]
#[path = "observation_tests.rs"]
mod tests;
