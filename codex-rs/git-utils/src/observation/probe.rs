//! The Git commands issued by an observation and how they are run.

use std::future::Future;
use std::path::Path;
use std::process::Output;

use tokio::process::Command;

use crate::FsmonitorOverride;
use crate::SAFE_BARE_REPOSITORY_CONFIG;
use crate::git_process::GitCommandBudget;
use crate::git_process::GitCommandError;
use crate::git_process::GitCommandOutputCap;
use crate::git_process::run_git_command_with_budget;
use crate::info::DISABLED_HOOKS_PATH;

/// Variables that select a repository, index, or object store other than the
/// one discovered from the observed directory (`git rev-parse
/// --local-env-vars`, plus `GIT_NAMESPACE`).
pub(super) const REPOSITORY_SELECTOR_VARIABLES: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_NAMESPACE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// Which end of the status command a HEAD sample was taken at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeadEndpoint {
    Initial,
    Final,
}

/// One Git command issued during an observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitProbe {
    WorktreeRoot,
    ObjectFormat,
    SymbolicRef(HeadEndpoint),
    HeadCommit(HeadEndpoint),
    RawHead(HeadEndpoint),
    Status,
}

impl GitProbe {
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::WorktreeRoot => &["rev-parse", "--show-toplevel"],
            Self::ObjectFormat => &["rev-parse", "--show-object-format=storage"],
            Self::SymbolicRef(_) => &["symbolic-ref", "-q", "HEAD"],
            Self::HeadCommit(_) => &[
                "rev-parse",
                "--verify",
                "-q",
                "--end-of-options",
                "HEAD^{commit}",
            ],
            Self::RawHead(_) => &["rev-parse", "--verify", "-q", "--end-of-options", "HEAD"],
            Self::Status => &[
                "status",
                "--porcelain=v2",
                "--branch",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=none",
                "--no-renames",
                "--no-ahead-behind",
            ],
        }
    }

    fn output_cap(self) -> GitCommandOutputCap {
        match self {
            Self::Status => GitCommandOutputCap::SharedAllowance,
            Self::WorktreeRoot
            | Self::ObjectFormat
            | Self::SymbolicRef(_)
            | Self::HeadCommit(_)
            | Self::RawHead(_) => GitCommandOutputCap::Metadata,
        }
    }
}

/// Runs the Git commands of an observation.
///
/// Production runs bounded subprocesses. Tests wrap it to change the
/// repository at a precise point between probes. Implementations report only
/// process and transport failures; exit statuses are interpreted by the caller.
pub(super) trait GitProbeRunner: Sync {
    fn run(
        &self,
        probe: GitProbe,
        cwd: &Path,
        budget: &GitCommandBudget,
    ) -> impl Future<Output = Result<Output, GitCommandError>> + Send;
}

pub(super) struct BoundedGitRunner;

impl GitProbeRunner for BoundedGitRunner {
    async fn run(
        &self,
        probe: GitProbe,
        cwd: &Path,
        budget: &GitCommandBudget,
    ) -> Result<Output, GitCommandError> {
        let mut command = Command::new("git");
        for variable in REPOSITORY_SELECTOR_VARIABLES {
            command.env_remove(variable);
        }
        command
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["-c", SAFE_BARE_REPOSITORY_CONFIG])
            .args(["-c", &format!("core.hooksPath={DISABLED_HOOKS_PATH}")])
            .args(["-c", FsmonitorOverride::Disabled.git_config_arg()])
            .args(probe.args())
            .current_dir(cwd);
        run_git_command_with_budget(&mut command, budget, probe.output_cap()).await
    }
}
