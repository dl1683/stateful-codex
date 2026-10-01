//! Git subprocess execution bounded by a shared deadline and output allowance.
//!
//! Unlike [`super::run_git_command_with_timeout_output`], output is charged
//! against the allowance before it is retained, so a large response fails with
//! [`GitCommandError::OutputLimit`] instead of being allocated and truncated.

use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::process::Output;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use tokio::io::AsyncRead;
use tokio::io::ReadBuf;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::Instant;
use tokio::time::timeout_at;

use super::GitProcessContainment;
use super::GitSpawnError;
use super::KillGitProcessTreeOnDrop;
use super::spawn_git_command;

/// Bytes read from a pipe per step; retained output grows only after charging.
const READ_CHUNK_BYTES: usize = 8 * 1024;
/// Combined stdout and stderr allowed for one metadata command.
const METADATA_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

/// A deadline and cumulative stdout-plus-stderr allowance shared by every
/// command run against it, including unsuccessful ones. Neither is ever reset.
#[derive(Debug)]
pub(crate) struct GitCommandBudget {
    deadline: Instant,
    remaining_output: AtomicUsize,
}

impl GitCommandBudget {
    pub(crate) fn new(deadline: Instant, output_allowance: usize) -> Self {
        Self {
            deadline,
            remaining_output: AtomicUsize::new(output_allowance),
        }
    }

    pub(crate) fn remaining_output(&self) -> usize {
        self.remaining_output.load(Ordering::SeqCst)
    }
}

/// The per-command output cap applied in addition to the shared allowance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitCommandOutputCap {
    /// Small metadata queries such as `rev-parse`, capped at 64 KiB.
    Metadata,
    /// Commands limited only by the shared allowance, such as `status`.
    SharedAllowance,
}

/// A process or transport failure. Exit statuses are left to the caller.
#[derive(Debug, thiserror::Error)]
pub(crate) enum GitCommandError {
    #[error(transparent)]
    Spawn(#[from] GitSpawnError),
    #[error("Git command exceeded its deadline")]
    Timeout,
    #[error("Git command exceeded its output allowance")]
    OutputLimit,
    #[error("failed to read Git command output: {0}")]
    Io(#[source] io::Error),
}

/// Runs `command` inside a required process-tree containment.
///
/// The deadline covers draining both pipes to EOF and waiting for exit, so a
/// descendant that keeps a pipe open cannot outlive it. Whenever this returns
/// or is dropped, every process in the contained tree is terminated.
pub(crate) async fn run_git_command_with_budget(
    command: &mut Command,
    budget: &GitCommandBudget,
    cap: GitCommandOutputCap,
) -> Result<Output, GitCommandError> {
    if Instant::now() >= budget.deadline {
        return Err(GitCommandError::Timeout);
    }
    let (child, process_tree) = spawn_git_command(command, GitProcessContainment::Required)?;
    collect_git_output(child, process_tree, budget, cap).await
}

async fn collect_git_output(
    mut child: Child,
    process_tree: KillGitProcessTreeOnDrop,
    budget: &GitCommandBudget,
    cap: GitCommandOutputCap,
) -> Result<Output, GitCommandError> {
    let command_remaining = AtomicUsize::new(match cap {
        GitCommandOutputCap::Metadata => METADATA_OUTPUT_LIMIT_BYTES,
        GitCommandOutputCap::SharedAllowance => usize::MAX,
    });
    let missing_pipe = || GitCommandError::Io(io::Error::other("Git output pipe was not captured"));
    let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
    let stderr = child.stderr.take().ok_or_else(missing_pipe)?;
    let collect = async {
        let (stdout, stderr, status) = tokio::try_join!(
            drain_pipe(stdout, budget, &command_remaining),
            drain_pipe(stderr, budget, &command_remaining),
            async { child.wait().await.map_err(GitCommandError::Io) },
        )?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    };
    let result = timeout_at(budget.deadline, collect)
        .await
        .unwrap_or(Err(GitCommandError::Timeout));
    // Read-only commands never hand surviving descendants to the caller.
    drop(process_tree);
    drop(child);
    result
}

async fn drain_pipe(
    mut pipe: impl AsyncRead + Unpin,
    budget: &GitCommandBudget,
    command_remaining: &AtomicUsize,
) -> Result<Vec<u8>, GitCommandError> {
    let mut retained = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        let mut read = ReadBuf::new(&mut chunk);
        poll_fn(|cx| Pin::new(&mut pipe).poll_read(cx, &mut read))
            .await
            .map_err(GitCommandError::Io)?;
        let bytes = read.filled();
        if bytes.is_empty() {
            return Ok(retained);
        }
        // Reading past an exhausted allowance distinguishes EOF from one more byte.
        if !try_charge(command_remaining, bytes.len())
            || !try_charge(&budget.remaining_output, bytes.len())
        {
            return Err(GitCommandError::OutputLimit);
        }
        retained.extend_from_slice(bytes);
    }
}

/// Consumes up to `bytes` from `remaining`, returning whether all were available.
fn try_charge(remaining: &AtomicUsize, bytes: usize) -> bool {
    let previous = match remaining.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
        Some(remaining.saturating_sub(bytes))
    }) {
        Ok(previous) | Err(previous) => previous,
    };
    previous >= bytes
}

#[cfg(test)]
#[path = "bounded_tests.rs"]
mod tests;
