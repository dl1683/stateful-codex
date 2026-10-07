use std::io;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;

use codex_protocol::shell_environment::scrub_non_inheritable_env_vars;
#[cfg(windows)]
use codex_utils_pty::JobObject;
#[cfg(unix)]
use codex_utils_pty::process_group::kill_process_group;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::timeout;

mod bounded;

pub(crate) use bounded::GitCommandBudget;
pub(crate) use bounded::GitCommandError;
pub(crate) use bounded::GitCommandOutputCap;
pub(crate) use bounded::run_git_command_with_budget;

/// How strictly a spawned Git process tree must be contained for later cleanup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitProcessContainment {
    /// Contain the tree when the platform allows it; otherwise run uncontained.
    BestEffort,
    /// Fail rather than run a process whose tree cannot be terminated.
    ///
    /// On Unix both policies place the child in its own process group. On
    /// Windows this requires a job object that descendants cannot leave.
    Required,
}

/// Why a Git process could not be started under the requested policy.
#[derive(Debug, thiserror::Error)]
pub(crate) enum GitSpawnError {
    /// The process could not be started. On Windows with required
    /// containment, this includes failing to start it inside its job.
    #[error("failed to start Git: {0}")]
    Spawn(#[source] io::Error),
    /// Process-tree containment could not be established.
    #[error("failed to contain Git process tree: {0}")]
    Containment(#[source] io::Error),
}

struct KillGitProcessTreeOnDrop {
    #[cfg(unix)]
    process_id: u32,
    #[cfg(windows)]
    job: Option<JobObject>,
    #[cfg(unix)]
    armed: bool,
}

#[cfg(unix)]
impl Drop for KillGitProcessTreeOnDrop {
    fn drop(&mut self) {
        if self.armed {
            let _ = kill_process_group(self.process_id);
        }
    }
}

fn spawn_git_command(
    command: &mut Command,
    containment: GitProcessContainment,
) -> Result<(Child, KillGitProcessTreeOnDrop), GitSpawnError> {
    scrub_non_inheritable_env_vars(command.as_std_mut());
    #[cfg(unix)]
    command.process_group(0);
    command.kill_on_drop(true);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    let (child, job) = match containment {
        GitProcessContainment::BestEffort => {
            JobObject::spawn_background(command).map_err(GitSpawnError::Spawn)?
        }
        GitProcessContainment::Required => {
            let job = JobObject::create_without_breakaway().map_err(GitSpawnError::Containment)?;
            let child = job.spawn_contained(command).map_err(GitSpawnError::Spawn)?;
            (child, Some(job))
        }
    };
    #[cfg(not(windows))]
    let child = match containment {
        GitProcessContainment::BestEffort | GitProcessContainment::Required => {
            command.spawn().map_err(GitSpawnError::Spawn)?
        }
    };

    let process_tree = KillGitProcessTreeOnDrop {
        #[cfg(unix)]
        process_id: child.id().ok_or_else(|| {
            GitSpawnError::Containment(io::Error::other("spawned Git process has no ID"))
        })?,
        #[cfg(windows)]
        job,
        #[cfg(unix)]
        armed: true,
    };

    Ok((child, process_tree))
}

async fn wait_for_git_command_with_timeout_output(
    child: Child,
    process_tree: KillGitProcessTreeOnDrop,
    timeout_duration: Duration,
) -> Option<Output> {
    #[cfg(unix)]
    let mut process_tree = process_tree;

    let result = timeout(timeout_duration, child.wait_with_output()).await;

    match result {
        Ok(Ok(output)) => {
            #[cfg(windows)]
            if let Some(job) = &process_tree.job {
                job.preserve_descendants().ok()?;
            }

            #[cfg(unix)]
            {
                process_tree.armed = false;
            }
            Some(output)
        }
        _ => None,
    }
}

pub(crate) async fn run_git_command_with_timeout_output(
    command: &mut Command,
    timeout_duration: Duration,
) -> Option<Output> {
    let (child, process_tree) =
        spawn_git_command(command, GitProcessContainment::BestEffort).ok()?;
    wait_for_git_command_with_timeout_output(child, process_tree, timeout_duration).await
}

#[cfg(test)]
#[path = "git_process_tests.rs"]
mod tests;
