use std::path::Path;
use std::process::Command;

use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::StatefulRunId;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::changed_unpinned;
use super::workspace_manifest;
use crate::acceptance_observation::MAX_OUTSTANDING_READERS;
use crate::acceptance_observation::OUTSTANDING_READERS;

fn roots(root: &Path) -> Vec<String> {
    vec![root.to_string_lossy().to_string()]
}

fn ledger() -> AcceptanceLedger {
    AcceptanceLedger::empty(StatefulRunId::parse("run").expect("run id"))
}

/// Rewrites a file with same-length bytes and restores its modification time.
fn rewrite_preserving_metadata(path: &Path, content: &str) {
    let modified = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .expect("mtime");
    std::fs::write(path, content).expect("rewrite");
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(modified))
        .expect("restore mtime");
}

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(arguments)
        .current_dir(root)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {arguments:?}");
}

#[tokio::test]
async fn content_not_metadata_identifies_a_plain_workspace() {
    let project = TempDir::new().expect("project");
    let root = project.path();
    std::fs::write(root.join("input.csv"), "1,2").expect("input");
    std::fs::create_dir(root.join("target")).expect("cache");
    let before = workspace_manifest(&roots(root)).await.expect("identity");
    assert_eq!(workspace_manifest(&roots(root)).await, Some(before.clone()));
    // A cache directory is not identity.
    std::fs::write(root.join("target/build.log"), "built").expect("cache write");
    let cached = workspace_manifest(&roots(root)).await.expect("identity");
    assert!(!changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&cached)
    ));
    // Same length, same modification time, different bytes: still a change.
    rewrite_preserving_metadata(&root.join("input.csv"), "3,4");
    let after = workspace_manifest(&roots(root)).await.expect("identity");
    assert!(changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&after)
    ));
    assert!(changed_unpinned(
        &roots(root),
        &ledger(),
        None,
        Some(&after)
    ));
}

#[tokio::test]
async fn a_git_workspace_is_identified_by_tracked_and_untracked_content() {
    let project = TempDir::new().expect("project");
    let root = project.path();
    git(root, &["init", "--quiet"]);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/parse.py"), "def parse(): pass\n").expect("source");
    std::fs::write(root.join("README.md"), "Parser\n").expect("readme");
    std::fs::write(root.join(".gitignore"), "build/\n").expect("ignore");
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "initial"]);
    let identity = || async { workspace_manifest(&roots(root)).await.expect("identity") };
    let before = identity().await;

    // An ignored build output is not identity.
    std::fs::create_dir(root.join("build")).expect("build");
    std::fs::write(root.join("build/out.o"), "object").expect("ignored write");
    assert!(!changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&identity().await)
    ));

    // A tracked file rewritten with metadata preserved is a change.
    rewrite_preserving_metadata(&root.join("src/parse.py"), "def parse(): fail\n");
    let rewritten = identity().await;
    assert!(changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&rewritten)
    ));
    rewrite_preserving_metadata(&root.join("src/parse.py"), "def parse(): pass\n");

    // A new untracked file is a change.
    std::fs::write(root.join("notes.txt"), "scratch").expect("untracked");
    let untracked = identity().await;
    assert!(changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&untracked)
    ));
    std::fs::remove_file(root.join("notes.txt")).expect("remove");

    // A commit changes repository state even when no file content changes.
    git(root, &["commit", "--quiet", "--allow-empty", "-m", "empty"]);
    assert!(changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&identity().await)
    ));
}

#[tokio::test]
async fn an_identity_without_a_free_worker_slot_is_unavailable() {
    let project = TempDir::new().expect("project");
    OUTSTANDING_READERS.store(MAX_OUTSTANDING_READERS, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(workspace_manifest(&roots(project.path())).await, None);
    OUTSTANDING_READERS.store(0, std::sync::atomic::Ordering::SeqCst);
    assert!(workspace_manifest(&roots(project.path())).await.is_some());
    assert_eq!(
        OUTSTANDING_READERS.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
