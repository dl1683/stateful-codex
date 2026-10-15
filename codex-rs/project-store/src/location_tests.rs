use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn absolute(path: &Path) -> AbsolutePathBuf {
    AbsolutePathBuf::from_absolute_path(path).unwrap()
}

fn seed_store(project: &Path) {
    fs::create_dir_all(project.join(".stateful")).unwrap();
    fs::write(project.join(".stateful/memory.sqlite"), b"fixture").unwrap();
}

#[test]
fn explicit_projects_have_independent_local_paths_without_creating_files() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let first_path = absolute(first.path()).canonicalize().unwrap();
    let second_path = absolute(second.path()).canonicalize().unwrap();
    assert_eq!(
        [first.path(), second.path()]
            .map(|root| ProjectStoreLocation::select_project(&absolute(root)).unwrap()),
        [first_path, second_path].map(|project_root| ProjectStoreLocation {
            database_path: project_root.join(".stateful/memory.sqlite"),
            project_root,
        })
    );
    assert!(!first.path().join(".stateful").exists());
    assert!(!second.path().join(".stateful").exists());
}

#[test]
fn git_directory_and_worktree_file_select_nearest_root() {
    for marker_is_file in [false, true] {
        let workspace = TempDir::new().unwrap();
        let root = workspace.path().join("repo");
        let child = root.join("src/deep");
        fs::create_dir_all(&child).unwrap();
        if marker_is_file {
            fs::write(root.join(".git"), "gitdir: elsewhere").unwrap();
        } else {
            fs::create_dir(root.join(".git")).unwrap();
        }
        assert_eq!(
            ProjectStoreLocation::resolve_workspace(&absolute(&child), &absolute(workspace.path()))
                .unwrap(),
            ProjectStoreLocation::select_project(&absolute(&root)).unwrap()
        );
    }
}

#[test]
fn discovers_store_in_non_git_project_without_using_ancestor_outside_boundary() {
    let outer = TempDir::new().unwrap();
    seed_store(outer.path());
    let project = outer.path().join("plain");
    let child = project.join("child");
    fs::create_dir_all(&child).unwrap();
    assert!(matches!(
        ProjectStoreLocation::resolve_workspace(&absolute(&child), &absolute(&project)),
        Err(ResolveError::ExplicitProjectRequired)
    ));
    seed_store(&project);
    assert_eq!(
        ProjectStoreLocation::resolve_workspace(&absolute(&child), &absolute(&project)).unwrap(),
        ProjectStoreLocation::select_project(&absolute(&project)).unwrap()
    );
}

#[test]
fn nested_stores_require_explicit_selection() {
    let outer = TempDir::new().unwrap();
    let inner = outer.path().join("inner");
    seed_store(outer.path());
    seed_store(&inner);
    assert!(matches!(
        ProjectStoreLocation::resolve_workspace(&absolute(&inner), &absolute(outer.path())),
        Err(ResolveError::AmbiguousProjects)
    ));
    assert_eq!(
        ProjectStoreLocation::select_project(&absolute(&inner))
            .unwrap()
            .project_root(),
        &absolute(&inner).canonicalize().unwrap()
    );
}

#[test]
fn outside_workspace_and_non_directory_selection_are_refused() {
    let workspace = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    assert!(matches!(
        ProjectStoreLocation::resolve_workspace(
            &absolute(outside.path()),
            &absolute(workspace.path())
        ),
        Err(ResolveError::OutsideWorkspace)
    ));
    let file = workspace.path().join("file");
    fs::write(&file, b"content").unwrap();
    assert!(matches!(
        ProjectStoreLocation::select_project(&absolute(&file)),
        Err(ResolveError::NotDirectory)
    ));
}

#[test]
fn malformed_store_components_are_refused() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join(".stateful"), b"not a directory").unwrap();
    assert!(matches!(
        ProjectStoreLocation::select_project(&absolute(root.path())),
        Err(ResolveError::UnsafeStorePath)
    ));
    let other = TempDir::new().unwrap();
    fs::create_dir_all(other.path().join(".stateful/memory.sqlite")).unwrap();
    assert!(matches!(
        ProjectStoreLocation::select_project(&absolute(other.path())),
        Err(ResolveError::UnsafeStorePath)
    ));
}

#[test]
fn store_directory_redirection_is_refused() {
    let root = TempDir::new().unwrap();
    let target = TempDir::new().unwrap();
    let link = root.path().join(".stateful");
    #[cfg(unix)]
    std::os::unix::fs::symlink(target.path(), &link).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(target.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "junction setup failed: {output:?}");
    }
    assert!(matches!(
        ProjectStoreLocation::select_project(&absolute(root.path())),
        Err(ResolveError::UnsafeStorePath)
    ));
}

#[test]
fn moved_project_resolves_without_retaining_the_old_location() {
    let workspace = TempDir::new().unwrap();
    let old = workspace.path().join("old");
    let new = workspace.path().join("new");
    seed_store(&old);
    let original = ProjectStoreLocation::select_project(&absolute(&old)).unwrap();
    fs::rename(&old, &new).unwrap();
    assert_eq!(
        ProjectStoreLocation::resolve_workspace(&absolute(&new), &absolute(workspace.path()))
            .unwrap(),
        ProjectStoreLocation::select_project(&absolute(&new)).unwrap()
    );
    assert!(!original.database_path().as_path().exists());
}

#[test]
fn canonical_workspace_boundary_rejects_redirected_child() {
    let workspace = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    seed_store(outside.path());
    let link = workspace.path().join("escape");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "junction setup failed: {output:?}");
    }
    assert!(matches!(
        ProjectStoreLocation::resolve_workspace(&absolute(&link), &absolute(workspace.path())),
        Err(ResolveError::OutsideWorkspace)
    ));
}
