use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::probe::REPOSITORY_SELECTOR_VARIABLES;
use super::*;

const GENEROUS: Duration = Duration::from_secs(60);

/// Runs a fixture Git command synchronously and returns its trimmed stdout.
fn git(cwd: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    for variable in REPOSITORY_SELECTOR_VARIABLES {
        command.env_remove(variable);
    }
    let output = command
        .args([
            "-c",
            "user.name=Codex",
            "-c",
            "user.email=codex@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("utf-8 output")
        .trim_end()
        .to_string()
}

/// Initializes a repository at `path` on `main`, with one committed file.
fn committed_repo(path: &Path) -> GitSha {
    std::fs::create_dir_all(path).expect("create repository directory");
    git(path, &["init", "-q", "-b", "main"]);
    std::fs::write(path.join("tracked.txt"), "one\n").expect("write tracked file");
    git(path, &["add", "tracked.txt"]);
    git(path, &["commit", "-q", "-m", "initial"]);
    GitSha::new(&git(path, &["rev-parse", "HEAD"]))
}

fn absolute(path: &Path) -> AbsolutePathBuf {
    // Git reports the resolved path, for example through macOS's /private.
    #[cfg(unix)]
    let path = path.canonicalize().expect("canonicalize fixture path");
    AbsolutePathBuf::from_absolute_path(path).expect("absolute fixture path")
}

fn generous_budget() -> GitCommandBudget {
    GitCommandBudget::new(Instant::now() + GENEROUS, OUTPUT_ALLOWANCE_BYTES)
}

async fn observe(root: &Path) -> GitRepositoryObservation {
    observe_with(&BoundedGitRunner, root, &generous_budget()).await
}

fn on_main(root: &Path, oid: GitSha, worktree: GitWorktreeObservation) -> GitRepositoryObservation {
    GitRepositoryObservation {
        worktree_root: Some(absolute(root)),
        head: GitHeadObservation::Commit {
            oid,
            head_ref: Some("refs/heads/main".to_string()),
        },
        worktree,
    }
}

#[tokio::test]
async fn committed_clean_checkout_is_clean() {
    let temp = TempDir::new().expect("tempdir");
    let oid = committed_repo(temp.path());
    assert_eq!(
        observe(temp.path()).await,
        on_main(temp.path(), oid, GitWorktreeObservation::Clean)
    );
}

#[tokio::test]
async fn every_kind_of_change_is_dirty() {
    for change in ["staged", "unstaged", "deleted", "untracked"] {
        let temp = TempDir::new().expect("tempdir");
        let repo = temp.path();
        let oid = committed_repo(repo);
        match change {
            "staged" => {
                std::fs::write(repo.join("staged.txt"), "new\n").expect("write");
                git(repo, &["add", "staged.txt"]);
            }
            "unstaged" => std::fs::write(repo.join("tracked.txt"), "two\n").expect("write"),
            "deleted" => std::fs::remove_file(repo.join("tracked.txt")).expect("delete"),
            _ => {
                std::fs::create_dir(repo.join("nested")).expect("mkdir");
                std::fs::write(repo.join("nested").join("new.txt"), "new\n").expect("write");
            }
        }
        assert_eq!(
            observe(repo).await,
            on_main(repo, oid, GitWorktreeObservation::Dirty),
            "{change}"
        );
    }
}

#[tokio::test]
async fn unborn_branch_is_reported_with_its_ref() {
    let temp = TempDir::new().expect("tempdir");
    git(temp.path(), &["init", "-q", "-b", "trunk"]);
    let unborn = |worktree| GitRepositoryObservation {
        worktree_root: Some(absolute(temp.path())),
        head: GitHeadObservation::Unborn {
            head_ref: "refs/heads/trunk".to_string(),
        },
        worktree,
    };
    assert_eq!(
        observe(temp.path()).await,
        unborn(GitWorktreeObservation::Clean)
    );

    std::fs::write(temp.path().join("new.txt"), "new\n").expect("write");
    assert_eq!(
        observe(temp.path()).await,
        unborn(GitWorktreeObservation::Dirty)
    );
}

#[tokio::test]
async fn detached_head_has_no_ref() {
    let temp = TempDir::new().expect("tempdir");
    let oid = committed_repo(temp.path());
    git(temp.path(), &["checkout", "-q", "--detach"]);
    assert_eq!(
        observe(temp.path()).await,
        GitRepositoryObservation {
            worktree_root: Some(absolute(temp.path())),
            head: GitHeadObservation::Commit {
                oid,
                head_ref: None
            },
            worktree: GitWorktreeObservation::Clean,
        }
    );
}

#[tokio::test]
async fn exhausted_budget_is_unknown_rather_than_clean() {
    let temp = TempDir::new().expect("tempdir");
    let budget = GitObservationBudget::until(Instant::now());
    let root = absolute(temp.path());
    assert_eq!(
        observe_repository(&root, &budget).await,
        GitRepositoryObservation {
            worktree_root: None,
            head: GitHeadObservation::Unknown(GitObservationFailure::Timeout),
            worktree: GitWorktreeObservation::Unknown(GitObservationFailure::Timeout),
        }
    );
}

#[tokio::test]
async fn large_status_output_is_unknown_rather_than_clean() {
    let temp = TempDir::new().expect("tempdir");
    committed_repo(temp.path());
    for index in 0..200 {
        let name = format!("untracked-file-with-a-long-descriptive-name-{index:04}.txt");
        std::fs::write(temp.path().join(name), "x").expect("write");
    }
    let budget = GitCommandBudget::new(Instant::now() + GENEROUS, 4 * 1024);
    let failure = GitObservationFailure::OutputLimit;
    assert_eq!(
        observe_with(&BoundedGitRunner, temp.path(), &budget).await,
        GitRepositoryObservation {
            worktree_root: Some(absolute(temp.path())),
            head: GitHeadObservation::Unknown(failure),
            worktree: GitWorktreeObservation::Unknown(failure),
        }
    );
}

/// How the repository changes immediately before the final HEAD sample.
#[derive(Clone, Copy)]
enum HeadChange {
    NewCommit,
    SameCommitBranchSwitch,
}

/// Forwards to real Git, changing HEAD once just before the final sample.
struct ChangeHeadBeforeFinalSample<'a> {
    repo: &'a Path,
    change: HeadChange,
    pending: AtomicBool,
}

impl GitProbeRunner for ChangeHeadBeforeFinalSample<'_> {
    async fn run(
        &self,
        probe: GitProbe,
        cwd: &Path,
        budget: &GitCommandBudget,
    ) -> Result<Output, GitCommandError> {
        if probe == GitProbe::SymbolicRef(HeadEndpoint::Final)
            && self.pending.swap(false, Ordering::SeqCst)
        {
            match self.change {
                HeadChange::NewCommit => {
                    git(self.repo, &["commit", "-q", "--allow-empty", "-m", "moved"]);
                }
                HeadChange::SameCommitBranchSwitch => {
                    git(self.repo, &["checkout", "-q", "-b", "other"]);
                }
            }
        }
        BoundedGitRunner.run(probe, cwd, budget).await
    }
}

#[tokio::test]
async fn head_changes_between_samples_are_unstable() {
    for change in [HeadChange::NewCommit, HeadChange::SameCommitBranchSwitch] {
        let temp = TempDir::new().expect("tempdir");
        committed_repo(temp.path());
        let runner = ChangeHeadBeforeFinalSample {
            repo: temp.path(),
            change,
            pending: AtomicBool::new(true),
        };
        let failure = GitObservationFailure::UnstableSample;
        assert_eq!(
            observe_with(&runner, temp.path(), &generous_budget()).await,
            GitRepositoryObservation {
                worktree_root: Some(absolute(temp.path())),
                head: GitHeadObservation::Unknown(failure),
                worktree: GitWorktreeObservation::Unknown(failure),
            }
        );
    }
}
