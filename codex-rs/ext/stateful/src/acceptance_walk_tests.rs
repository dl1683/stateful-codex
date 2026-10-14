use super::super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[path = "acceptance_walk_platform_tests.rs"]
mod platform;

fn snapshot(root: &Path) -> Result<Manifest, IdentityGap> {
    let root = root.canonicalize().unwrap();
    identify(&[root], LIMITS, Instant::now() + LIMITS.budget)
}

fn empty_ledger() -> AcceptanceLedger {
    AcceptanceLedger::empty(codex_stateful_runtime::StatefulRunId::parse("walk").unwrap())
}

#[test]
fn w4_identical_rewrites_are_equal_but_content_changes_are_not() {
    let project = TempDir::new().unwrap();
    let file = project.path().join("input");
    std::fs::write(&file, "first").unwrap();
    let before = snapshot(project.path()).unwrap();
    std::fs::write(&file, "first").unwrap();
    let same = snapshot(project.path()).unwrap();
    assert_eq!(before, same);
    assert_eq!(
        changed_unpinned(&empty_ledger(), Ok(&before), Ok(&same)),
        None
    );
    std::fs::write(&file, "other").unwrap();
    let after = snapshot(project.path()).unwrap();
    assert_eq!(
        changed_unpinned(&empty_ledger(), Ok(&before), Ok(&after)),
        Some(IdentityChange::Path(file.canonicalize().unwrap()))
    );
}

#[test]
fn w5_parent_repository_is_not_workspace_state() {
    let parent = super::repository();
    let project = parent.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("input"), "input").unwrap();
    let before = snapshot(&project).unwrap();
    super::git(
        parent.path(),
        &["commit", "--quiet", "--allow-empty", "-m", "parent"],
    );
    assert_eq!(snapshot(&project).unwrap(), before);
}

#[test]
fn w6_exclusions_apply_to_directories_at_every_depth() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    std::fs::write(root.join("target"), "a file").unwrap();
    let before = snapshot(root).unwrap();
    for name in [
        ".hg",
        ".svn",
        "node_modules",
        "target",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".venv",
        "venv",
        ".tox",
        ".blackboard",
    ] {
        let excluded = root.join("pkg").join(name);
        std::fs::create_dir_all(&excluded).unwrap();
        std::fs::write(excluded.join("output"), "ignored").unwrap();
    }
    assert_eq!(snapshot(root).unwrap(), before);
    assert_eq!(
        before.files.keys().cloned().collect::<Vec<_>>(),
        vec![root.canonicalize().unwrap().join("target")]
    );
}

#[test]
fn w6_git_excludes_only_untracked_directory_components() {
    let project = super::repository();
    let root = project.path();
    std::fs::create_dir(root.join("target")).unwrap();
    std::fs::write(root.join("target/tracked"), "tracked").unwrap();
    super::git(root, &["add", "target/tracked"]);
    let before = snapshot(root).unwrap();
    assert!(
        before
            .files
            .contains_key(&root.canonicalize().unwrap().join("target/tracked"))
    );
    for name in ["node_modules", ".venv", ".blackboard", "target"] {
        std::fs::create_dir_all(root.join("pkg").join(name)).unwrap();
        std::fs::write(root.join("pkg").join(name).join("output"), "ignored").unwrap();
    }
    assert_eq!(snapshot(root).unwrap(), before);
    std::fs::write(root.join("target/tracked"), "changed").unwrap();
    assert_ne!(snapshot(root).unwrap(), before);
}

#[test]
fn w6_embedded_git_files_and_empty_repositories_are_gaps() {
    for directory in [false, true] {
        let project = TempDir::new().unwrap();
        std::fs::create_dir(project.path().join("child")).unwrap();
        let git = project.path().join("child/.git");
        if directory {
            std::fs::create_dir(&git).unwrap();
        } else {
            std::fs::write(&git, "gitdir: elsewhere").unwrap();
        }
        assert_eq!(
            snapshot(project.path()),
            Err(IdentityGap::Entry {
                path: project.path().canonicalize().unwrap().join("child/.git"),
                kind: EntryKind::EmbeddedRepository,
            })
        );
    }
}

#[tokio::test]
async fn w11_empty_walk_root_disappearance_is_a_roots_gap() {
    let holder = TempDir::new().unwrap();
    let root = holder.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let roots = vec![root.display().to_string()];
    let before = workspace_manifest(&roots).await.unwrap();
    assert_eq!(snapshot(&root).unwrap(), before);
    std::fs::remove_dir(&root).unwrap();
    let after = workspace_manifest(&roots).await;
    assert_eq!(after, Err(IdentityGap::Roots));
    assert_eq!(
        changed_unpinned(&empty_ledger(), Ok(&before), after.as_ref()),
        Some(IdentityChange::Gap(Side::End, IdentityGap::Roots))
    );
}

#[test]
fn walk_links_use_their_own_key_and_target_content() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    std::fs::write(root.join("data"), "first").unwrap();
    let alias = root.join("alias");
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink("data", &alias);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file("data", &alias);
    if let Err(error) = result {
        eprintln!("not run (preflight): file symlink: {error}");
        return;
    }
    let before = snapshot(&root).unwrap();
    assert_eq!(before.files.len(), 2);
    std::fs::write(root.join("data"), "other").unwrap();
    let after = snapshot(&root).unwrap();
    assert_ne!(before.files[&alias], after.files[&alias]);
    std::fs::remove_file(&alias).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("missing", &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file("missing", &alias).unwrap();
    assert_eq!(
        snapshot(&root),
        Err(IdentityGap::Entry {
            path: alias.clone(),
            kind: EntryKind::EscapingLink
        })
    );
    std::fs::remove_file(&alias).unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("data"), "outside").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("data"), &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(outside.path().join("data"), &alias).unwrap();
    assert_eq!(
        snapshot(&root),
        Err(IdentityGap::Entry {
            path: alias.clone(),
            kind: EntryKind::EscapingLink,
        })
    );
    std::fs::remove_file(&alias).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("alias", &alias).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file("alias", &alias).unwrap();
    assert_eq!(
        snapshot(&root),
        Err(IdentityGap::Entry {
            path: alias,
            kind: EntryKind::EscapingLink,
        })
    );
}

#[test]
fn walk_directory_links_are_not_entered_even_with_an_excluded_name() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("real")).unwrap();
    let alias = root.join(".venv");
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink("real", &alias);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir("real", &alias);
    if let Err(error) = result {
        eprintln!("not run (preflight): directory symlink: {error}");
        return;
    }
    assert_eq!(
        snapshot(&root),
        Err(IdentityGap::Entry {
            path: alias,
            kind: EntryKind::DirectoryLink
        })
    );
}

#[test]
fn manifest_order_does_not_depend_on_creation_order() {
    let project = TempDir::new().unwrap();
    let root = project.path().canonicalize().unwrap();
    for name in ["z", "b", "a"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let before = snapshot(&root).unwrap();
    assert_eq!(
        before.files.keys().cloned().collect::<Vec<_>>(),
        ["a", "b", "z"].map(|name| root.join(name)).to_vec()
    );
    for name in ["a", "b", "z"] {
        std::fs::remove_file(root.join(name)).unwrap();
        std::fs::write(root.join(name), name).unwrap();
    }
    assert_eq!(snapshot(&root).unwrap(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w2_retained_readers_refuse_a_fifth_walk_identity_then_recover() {
    use crate::acceptance_identity_worker::read_identity;
    use crate::acceptance_observation::OUTSTANDING_READERS;
    use std::sync::atomic::Ordering;
    let project = TempDir::new().unwrap();
    let roots = vec![project.path().display().to_string()];
    let baseline = OUTSTANDING_READERS.load(Ordering::SeqCst);
    let mut releases = Vec::new();
    for _ in 0..4 {
        let (release, blocked) = std::sync::mpsc::channel();
        let (entered, ready) = std::sync::mpsc::channel();
        releases.push(release);
        let worker = tokio::spawn(read_identity(
            Duration::from_millis(/*millis*/ 100),
            move |_| {
                entered.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(())
            },
        ));
        ready.recv_timeout(Duration::from_secs(/*secs*/ 5)).unwrap();
        assert_eq!(worker.await.unwrap(), Err(IdentityGap::Time));
    }
    let fifth = workspace_manifest(&roots).await;
    for release in releases {
        release.send(()).unwrap();
    }
    for _ in 0..100 {
        if OUTSTANDING_READERS.load(Ordering::SeqCst) == baseline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(/*millis*/ 10)).await;
    }
    assert_eq!(fifth, Err(IdentityGap::Slots));
    assert_eq!(OUTSTANDING_READERS.load(Ordering::SeqCst), baseline);
    assert!(workspace_manifest(&roots).await.is_ok());
}
