use super::*;
use pretty_assertions::assert_eq;

fn snapshot(root: &Path) -> super::super::Manifest {
    identify(&canonical(root), LIMITS).expect("identity")
}

fn differs(before: &super::super::Manifest, after: &super::super::Manifest, root: &Path) -> bool {
    changed_unpinned(&roots(root), &ledger(), Some(before), Some(after))
}

#[tokio::test]
async fn w11_missing_second_root_is_not_dropped() {
    let project = repository();
    let mut roots = roots(project.path());
    roots.push(project.path().join("missing").display().to_string());
    assert_eq!(crate::acceptance_workspace::workspace_manifest(&roots).await, Err(IdentityGap::Roots));
    roots.pop();
    roots.push(project.path().join("README.md").display().to_string());
    assert_eq!(crate::acceptance_workspace::workspace_manifest(&roots).await, Err(IdentityGap::Roots));
}

#[test]
fn w12_refs_and_symbolic_head_are_identity() {
    let project = repository();
    let root = project.path();
    let before = snapshot(root);
    git(root, &["branch", "other"]);
    let created = snapshot(root);
    let creation = differs(&before, &created, root);
    git(root, &["branch", "-D", "other"]);
    let deletion = differs(&created, &snapshot(root), root);
    git(root, &["branch", "other"]);
    let before = snapshot(root);
    git(root, &["switch", "--quiet", "other"]);
    let symbolic = differs(&before, &snapshot(root), root);
    git(root, &["commit", "--quiet", "--allow-empty", "-m", "other"]);
    git(root, &["switch", "--quiet", "-"]);
    let before_move = snapshot(root);
    git(root, &["branch", "-f", "other", "HEAD"]);
    let movement = differs(&before_move, &snapshot(root), root);
    git(root, &["symbolic-ref", "refs/alias", "refs/heads/other"]);
    let alias = snapshot(root);
    let active = git(root, &["symbolic-ref", "HEAD"]);
    git(root, &["symbolic-ref", "refs/alias", &active]);
    let retarget = differs(&alias, &snapshot(root), root);
    git(root, &["checkout", "--quiet", "--detach"]);
    let detached = differs(&before_move, &snapshot(root), root);
    assert_eq!([creation, deletion, symbolic, movement, detached, retarget], [true; 6]);
}

#[test]
fn w18_stable_absence_and_both_transitions() {
    let project = repository();
    let root = project.path();
    let present = snapshot(root);
    std::fs::remove_file(root.join("README.md")).expect("delete");
    let absent = snapshot(root);
    assert!(differs(&present, &absent, root), "present to absent");
    assert!(!differs(&absent, &snapshot(root), root), "stable absence");
    std::fs::write(root.join("README.md"), "Parser\n").expect("restore");
    assert!(differs(&absent, &snapshot(root), root), "absent to present");
    std::fs::remove_dir_all(root.join("src")).expect("missing ancestor");
    let absent = snapshot(root);
    assert!(!differs(&absent, &snapshot(root), root), "missing ancestor");
}

#[test]
fn w18_dangling_ancestor_is_not_absence() {
    let project = repository();
    let root = project.path();
    std::fs::remove_dir_all(root.join("src")).unwrap();
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink("missing", root.join("src"));
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir("missing", root.join("src"));
    if let Err(error) = result {
        eprintln!("not run (preflight): dangling directory symlink: {error}");
        return;
    }
    assert_eq!(crate::acceptance_workspace::identify(&canonical(root), LIMITS, std::time::Instant::now() + LIMITS.budget), Err(IdentityGap::Entry {
        path: root.canonicalize().unwrap().join("src"),
        kind: crate::acceptance_identity_reason::EntryKind::Reparse,
    }));
}

#[test]
fn w14_empty_git_root_is_identified() {
    let project = TempDir::new().expect("project");
    git(project.path(), &["init", "--quiet"]);
    let first = snapshot(project.path());
    assert!(!differs(&first, &snapshot(project.path()), project.path()));
}

fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(target, link);
    if let Err(error) = result {
        eprintln!("not run (preflight): file symlink: {error}");
        return false;
    }
    true
}

#[test]
fn w7_w8_links_are_contained_and_include_target_content() {
    let project = repository();
    let root = project.path();
    let link = root.join("alias");
    if !link_file(Path::new("README.md"), &link) {
        return;
    }
    let before = snapshot(root);
    std::fs::write(root.join("README.md"), "changed").expect("target");
    let after = snapshot(root);
    let target_content = before.files.get(&root.canonicalize().unwrap().join("alias")) != after.files.get(&root.canonicalize().unwrap().join("alias"));
    std::fs::remove_file(&link).expect("remove link");
    let outside = TempDir::new().expect("outside");
    std::fs::write(outside.path().join("data"), "outside").expect("outside file");
    assert!(link_file(&outside.path().join("data"), &link));
    let escaping = identify(&canonical(root), LIMITS).is_none();
    std::fs::remove_file(&link).expect("remove link");
    assert!(link_file(Path::new("missing"), &link));
    let dangling = identify(&canonical(root), LIMITS).is_none();
    assert_eq!([target_content, escaping, dangling], [true; 3]);
}

#[test]
fn w11_root_retargeting_changes_keys() {
    let first = repository();
    let second = repository();
    let holder = TempDir::new().expect("holder");
    let link = holder.path().join("root");
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(first.path(), &link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(first.path(), &link);
    if let Err(error) = result {
        eprintln!("not run (preflight): directory symlink: {error}");
        return;
    }
    let before = snapshot(&link);
    #[cfg(unix)]
    {
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(second.path(), &link).unwrap();
    }
    #[cfg(windows)]
    {
        std::fs::remove_dir(&link).unwrap();
        std::os::windows::fs::symlink_dir(second.path(), &link).unwrap();
    }
    assert!(differs(&before, &snapshot(&link), &link));
}

#[test]
fn w7_unreadable_file_is_refused_after_preflight() {
    let project = repository();
    let path = project.path().join("README.md");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap();
        assert!(Command::new("icacls").arg(&path).args(["/deny", &format!("{user}:(R)")]).status().unwrap().success());
    }
    let denied = std::fs::File::open(&path).is_err();
    let identity = crate::acceptance_workspace::identify(&canonical(project.path()), LIMITS, std::time::Instant::now() + LIMITS.budget);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    #[cfg(windows)]
    {
        assert!(Command::new("icacls").arg(&path).args(["/remove:d", &std::env::var("USERNAME").unwrap()]).status().unwrap().success());
    }
    if !denied {
        eprintln!("not run (preflight): read denial does not take effect");
        return;
    }
    assert_eq!(identity, Err(IdentityGap::Entry {
        path: path.canonicalize().unwrap(),
        kind: crate::acceptance_identity_reason::EntryKind::Unreadable,
    }));
}

#[cfg(windows)]
#[test]
fn w7_escaping_junction_is_refused_after_preflight() {
    let project = repository();
    let root = project.path();
    std::fs::write(root.join("src/data"), "inside").expect("inside");
    git(root, &["add", "."]);
    std::fs::remove_dir_all(root.join("src")).expect("remove src");
    let outside = TempDir::new().expect("outside");
    std::fs::write(outside.path().join("data"), "outside").expect("outside");
    let status = Command::new("cmd").args(["/C", "mklink", "/J"]).arg(root.join("src")).arg(outside.path()).status().expect("junction preflight");
    assert!(status.success(), "junction preflight");
    assert_eq!(crate::acceptance_workspace::identify(&canonical(root), LIMITS, std::time::Instant::now() + LIMITS.budget), Err(IdentityGap::Entry {
        path: root.canonicalize().unwrap().join("src"),
        kind: crate::acceptance_identity_reason::EntryKind::Reparse,
    }));
}
