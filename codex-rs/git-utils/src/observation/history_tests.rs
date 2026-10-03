use std::path::Path;
use std::time::Duration;

use codex_protocol::protocol::GitSha;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::Instant;

use super::GitChangedPath;
use super::GitCommitRange;
use super::GitWorktreePaths;
use super::changed_paths;
use super::commit_patch;
use super::commits_between;
use super::staged_changes;
use crate::observation::GitObservationBudget;
use crate::observation::probe::REPOSITORY_SELECTOR_VARIABLES;

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

fn commit(path: &Path, file: &str, message: &str) -> GitSha {
    std::fs::write(path.join(file), message).expect("write file");
    git(path, &["add", file]);
    git(path, &["commit", "-q", "-m", message]);
    GitSha::new(&git(path, &["rev-parse", "HEAD"]))
}

fn budget() -> GitObservationBudget {
    GitObservationBudget::until(Instant::now() + Duration::from_secs(2))
}

#[tokio::test]
async fn commits_between_lists_new_commits_and_detects_discontinuity() {
    let temp_dir = TempDir::new().expect("tempdir");
    let path = temp_dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    let base = commit(path, "a.txt", "initial");
    commit(
        path,
        "b.txt",
        "Rename suggest_typos to typo_hints\n\nMatches the key our downstream app uses.",
    );
    let head = commit(path, "c.txt", "Make hints opt-in");
    let root = AbsolutePathBuf::from_absolute_path(path).expect("absolute");

    let advanced = commits_between(&root, &base, &head, 1, &budget()).await;
    let GitCommitRange::Advanced { commits, omitted } = advanced else {
        panic!("expected advanced history, got {advanced:?}");
    };
    git(path, &["checkout", "-q", "-b", "other", &base.0]);
    let other = commit(path, "d.txt", "Unrelated");
    let discontinuous = commits_between(&root, &head, &other, 10, &budget()).await;
    let all = commits_between(&root, &base, &head, 10, &budget()).await;
    let bodies = match all {
        GitCommitRange::Advanced { commits, .. } => commits
            .into_iter()
            .map(|commit| (commit.subject, commit.body))
            .collect::<Vec<_>>(),
        other => panic!("expected advanced history, got {other:?}"),
    };

    assert_eq!(
        (
            commits
                .iter()
                .map(|commit| commit.subject.clone())
                .collect::<Vec<_>>(),
            omitted,
            discontinuous,
            commits_between(&root, &head, &head, 10, &budget()).await,
            bodies,
        ),
        (
            vec!["Make hints opt-in".to_string()],
            true,
            GitCommitRange::Discontinuous,
            GitCommitRange::Unchanged,
            vec![
                ("Make hints opt-in".to_string(), String::new()),
                (
                    "Rename suggest_typos to typo_hints".to_string(),
                    "Matches the key our downstream app uses.".to_string()
                ),
            ],
        )
    );
}

#[tokio::test]
async fn changed_paths_lists_tracked_and_untracked_changes_within_a_limit() {
    let temp_dir = TempDir::new().expect("tempdir");
    let path = temp_dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    commit(path, "tracked.txt", "initial");
    std::fs::write(path.join("tracked.txt"), "edited").expect("edit");
    std::fs::write(path.join("notes.md"), "Priya asked for per=w").expect("untracked");
    let root = AbsolutePathBuf::from_absolute_path(path).expect("absolute");

    std::fs::write(path.join("staged.txt"), "staged").expect("staged");
    git(path, &["add", "staged.txt"]);
    let mut all = changed_paths(&root, 10, &budget()).await.expect("paths");
    all.paths.sort_by(|left, right| left.path.cmp(&right.path));
    assert_eq!(
        (
            all,
            changed_paths(&root, 1, &budget())
                .await
                .expect("paths")
                .omitted
        ),
        (
            GitWorktreePaths {
                paths: vec![
                    GitChangedPath {
                        status: "??".to_string(),
                        path: "notes.md".to_string(),
                    },
                    GitChangedPath {
                        status: "A ".to_string(),
                        path: "staged.txt".to_string(),
                    },
                    GitChangedPath {
                        status: " M".to_string(),
                        path: "tracked.txt".to_string(),
                    },
                ],
                omitted: false,
            },
            true
        )
    );
}

/// Message text cannot break the record framing: separator-like characters and extra
/// lines stay inside their own commit.
#[tokio::test]
async fn messages_with_separator_characters_keep_their_framing() {
    let temp_dir = TempDir::new().expect("tempdir");
    let path = temp_dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    let base = commit(path, "a.txt", "initial");
    let head = commit(
        path,
        "b.txt",
        "Odd \u{1e} subject \u{1f} here\n\nFirst body line.\n\u{1e}\u{1f}Second body line.",
    );
    let root = AbsolutePathBuf::from_absolute_path(path).expect("absolute");
    let GitCommitRange::Advanced { commits, omitted } =
        commits_between(&root, &base, &head, 10, &budget()).await
    else {
        panic!("expected advanced history");
    };
    assert_eq!(
        (
            commits.len(),
            omitted,
            commits[0].oid.clone(),
            commits[0].subject.clone(),
            commits[0].body.contains("Second body line."),
        ),
        (
            1,
            false,
            head.0,
            "Odd \u{1e} subject \u{1f} here".to_string(),
            true
        )
    );
}

#[tokio::test]
async fn staged_changes_name_the_staged_blob() {
    let temp_dir = TempDir::new().expect("tempdir");
    let path = temp_dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    commit(path, "tracked.txt", "initial");
    let root = AbsolutePathBuf::from_absolute_path(path).expect("absolute");
    let clean = staged_changes(&root, &budget()).await.expect("staged");
    std::fs::write(path.join("tracked.txt"), "first staging").expect("edit");
    git(path, &["add", "tracked.txt"]);
    let first = staged_changes(&root, &budget()).await.expect("staged");
    std::fs::write(path.join("tracked.txt"), "second staging").expect("edit");
    git(path, &["add", "tracked.txt"]);
    let second = staged_changes(&root, &budget()).await.expect("staged");
    assert_eq!(
        (
            clean.is_empty(),
            first.contains("tracked.txt"),
            first == second
        ),
        (true, true, false)
    );
}

#[tokio::test]
async fn commit_patch_shows_what_one_commit_changed() {
    let temp = TempDir::new().expect("tempdir");
    let path = temp.path();
    git(path, &["init", "-q"]);
    commit(
        path,
        "units.py",
        "YEARS = \"y\"
",
    );
    let changed = commit(
        path,
        "units.py",
        "YEARS = \"yr\"
",
    );
    let root = AbsolutePathBuf::from_absolute_path(path).expect("absolute path");

    let patch = commit_patch(&root, &changed, &budget())
        .await
        .expect("patch");

    let lines = patch
        .lines()
        .filter(|line| line.starts_with(['+', '-']))
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        vec![
            "--- a/units.py",
            "+++ b/units.py",
            "-YEARS = \"y\"",
            "+YEARS = \"yr\"",
        ]
    );
}
