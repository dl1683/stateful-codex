use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::StatefulRunId;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::LIMITS;
use super::Limits;
use super::changed_unpinned;
use super::identify;
use super::workspace_manifest;
use crate::acceptance_observation::MAX_OUTSTANDING_READERS;
use crate::acceptance_observation::OUTSTANDING_READERS;

fn roots(root: &Path) -> Vec<String> {
    vec![root.to_string_lossy().to_string()]
}

fn canonical(root: &Path) -> Vec<PathBuf> {
    vec![std::fs::canonicalize(root).expect("canonical root")]
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

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(arguments)
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A committed repository with sources, an ignore rule and ordinary files whose names mimic
/// repository-state keys.
fn repository() -> TempDir {
    let project = TempDir::new().expect("project");
    let root = project.path();
    git(root, &["init", "--quiet"]);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("src/parse.py"), "def parse(): pass\n").expect("source");
    std::fs::write(root.join("README.md"), "Parser\n").expect("readme");
    std::fs::write(root.join(".gitignore"), "build/\n").expect("ignore");
    std::fs::write(root.join(".git#HEAD"), "ordinary").expect("lookalike");
    std::fs::write(root.join(".git#index"), "ordinary").expect("lookalike");
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "initial"]);
    project
}

#[tokio::test]
async fn only_a_git_work_tree_can_be_identified() {
    // A plain project root is unavailable, even when a normal repository is nested in it.
    let plain = TempDir::new().expect("plain");
    std::fs::write(plain.path().join("input.csv"), "1,2").expect("input");
    assert_eq!(workspace_manifest(&roots(plain.path())).await, None);
    let child = plain.path().join("child");
    std::fs::create_dir(&child).expect("child");
    git(&child, &["init", "--quiet"]);
    assert_eq!(workspace_manifest(&roots(plain.path())).await, None);

    // A git failure (here a broken `.git` link) is unavailable, never a plain workspace.
    let broken = TempDir::new().expect("broken");
    std::fs::write(broken.path().join(".git"), "gitdir: missing-directory\n").expect("link");
    assert_eq!(workspace_manifest(&roots(broken.path())).await, None);

    // A repository without commits is identified, its unborn HEAD verified; the first commit
    // changes its state.
    let unborn = TempDir::new().expect("unborn");
    git(unborn.path(), &["init", "--quiet"]);
    std::fs::write(unborn.path().join("input.csv"), "1,2").expect("input");
    let before = workspace_manifest(&roots(unborn.path()))
        .await
        .expect("unborn identity");
    git(unborn.path(), &["add", "."]);
    git(unborn.path(), &["commit", "--quiet", "-m", "first"]);
    let after = workspace_manifest(&roots(unborn.path()))
        .await
        .expect("identity");
    assert!(changed_unpinned(
        &roots(unborn.path()),
        &ledger(),
        Some(&before),
        Some(&after)
    ));
    assert!(changed_unpinned(
        &roots(unborn.path()),
        &ledger(),
        None,
        Some(&after)
    ));
}

#[test]
fn a_git_workspace_is_identified_by_content_and_typed_repository_state() {
    let project = repository();
    let root = project.path();
    let identity = || identify(&canonical(root), LIMITS).expect("identity");
    let changed = |before: &super::Manifest| {
        changed_unpinned(&roots(root), &ledger(), Some(before), Some(&identity()))
    };
    let before = identity();

    // An ignored build output is not identity.
    std::fs::create_dir(root.join("build")).expect("build");
    std::fs::write(root.join("build/out.o"), "object").expect("ignored write");
    assert!(!changed(&before));

    // A tracked file rewritten with metadata preserved is a change.
    rewrite_preserving_metadata(&root.join("src/parse.py"), "def parse(): fail\n");
    assert!(changed(&before));
    rewrite_preserving_metadata(&root.join("src/parse.py"), "def parse(): pass\n");
    assert!(!changed(&before));

    // A new untracked file is a change.
    std::fs::write(root.join("notes.txt"), "scratch").expect("untracked");
    assert!(changed(&before));
    std::fs::remove_file(root.join("notes.txt")).expect("remove");

    // Repository state is typed, so ordinary files named like its keys cannot mask it: a
    // commit that changes no file and an index-only change are both changes.
    git(root, &["commit", "--quiet", "--allow-empty", "-m", "empty"]);
    assert!(changed(&before));
    let committed = identity();
    git(root, &["rm", "--cached", "--quiet", "README.md"]);
    assert!(changed(&committed));
}

#[test]
fn submodules_and_nested_repositories_make_the_identity_unavailable() {
    let project = repository();
    let root = project.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    git(
        root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head},vendor"),
        ],
    );
    assert_eq!(identify(&canonical(root), LIMITS), None, "a gitlink");

    let nested = repository();
    let nested_child = nested.path().join("child");
    std::fs::create_dir(&nested_child).expect("child");
    git(&nested_child, &["init", "--quiet"]);
    std::fs::write(nested_child.join("input.csv"), "1,2").expect("child file");
    assert_eq!(
        identify(&canonical(nested.path()), LIMITS),
        None,
        "a nested repository"
    );
}

#[test]
fn a_parent_tracked_file_inside_an_embedded_repository_is_unavailable() {
    // The parent tracks `child/input.csv` as a regular file before `child` becomes a
    // repository of its own, whose state the parent's identity would otherwise omit.
    let project = repository();
    let root = project.path();
    let child = root.join("child");
    std::fs::create_dir_all(child.join("deep")).expect("child");
    std::fs::write(
        child.join("input.csv"),
        "1,2
",
    )
    .expect("child input");
    std::fs::write(
        child.join("deep/data.csv"),
        "3
",
    )
    .expect("deep input");
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "child inputs"]);
    assert!(
        identify(&canonical(root), LIMITS).is_some(),
        "an ordinary hierarchy"
    );

    git(&child, &["init", "--quiet"]);
    git(&child, &["add", "input.csv"]);
    git(&child, &["commit", "--quiet", "-m", "child"]);
    assert_eq!(
        identify(&canonical(root), LIMITS),
        None,
        "an embedded repository"
    );

    // A `.git` file (a linked work tree or separated repository) is a boundary too.
    let linked = repository();
    let deep = linked.path().join("a/b");
    std::fs::create_dir_all(&deep).expect("deep");
    std::fs::write(
        deep.join("data.csv"),
        "3
",
    )
    .expect("deep input");
    git(linked.path(), &["add", "."]);
    git(linked.path(), &["commit", "--quiet", "-m", "deep input"]);
    std::fs::write(
        linked.path().join("a/.git"),
        "gitdir: elsewhere
",
    )
    .expect("git file");
    assert_eq!(
        identify(&canonical(linked.path()), LIMITS),
        None,
        "a .git file"
    );
}

#[test]
fn enumeration_stops_at_its_bounds() {
    let project = repository();
    let root = project.path();
    for index in 0..10 {
        std::fs::write(root.join(format!("file{index}.txt")), "x").expect("file");
    }
    assert!(identify(&canonical(root), LIMITS).is_some());
    let few_entries = Limits {
        entries: 3,
        ..LIMITS
    };
    assert_eq!(identify(&canonical(root), few_entries), None, "entries");
    let little_output = Limits {
        git_output: 16,
        ..LIMITS
    };
    assert_eq!(
        identify(&canonical(root), little_output),
        None,
        "git output"
    );
    let few_bytes = Limits { bytes: 4, ..LIMITS };
    assert_eq!(identify(&canonical(root), few_bytes), None, "hashed bytes");
    let no_time = Limits {
        budget: Duration::ZERO,
        ..LIMITS
    };
    assert_eq!(identify(&canonical(root), no_time), None, "deadline");
}

/// A tracked name that is not UTF-8 is identified by its exact bytes.
#[cfg(unix)]
#[test]
fn raw_byte_names_are_identified_exactly() {
    use std::os::unix::ffi::OsStringExt;
    let project = repository();
    let root = project.path();
    let name = PathBuf::from(std::ffi::OsString::from_vec(b"input\xff.csv".to_vec()));
    std::fs::write(root.join(&name), "1,2").expect("raw name");
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "-m", "raw"]);
    let before = identify(&canonical(root), LIMITS).expect("identity");
    std::fs::write(root.join(&name), "3,4").expect("rewrite");
    let after = identify(&canonical(root), LIMITS).expect("identity");
    assert!(changed_unpinned(
        &roots(root),
        &ledger(),
        Some(&before),
        Some(&after)
    ));
}

#[tokio::test]
async fn an_identity_without_a_free_worker_slot_is_unavailable() {
    let project = repository();
    OUTSTANDING_READERS.store(MAX_OUTSTANDING_READERS, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(workspace_manifest(&roots(project.path())).await, None);
    OUTSTANDING_READERS.store(0, std::sync::atomic::Ordering::SeqCst);
    assert!(workspace_manifest(&roots(project.path())).await.is_some());
    assert_eq!(
        OUTSTANDING_READERS.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
