use std::fs;
use std::process::Command;

use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

#[test]
fn private_storage_is_excluded_without_relying_on_git_ignores() {
    for git in [false, true] {
        let root = TempDir::new().expect("project");
        fs::write(root.path().join(".ignore"), "!.stateful/\n!.stateful/**\n!.ordinary\n!.stateful-notes\n!.STATEFUL/\n!.STATEFUL/**\n").expect("whitelist hidden controls");
        if git {
            assert!(
                Command::new("git")
                    .arg("init")
                    .arg(root.path())
                    .output()
                    .expect("git")
                    .status
                    .success()
            );
        }
        for name in [
            ".stateful/memory.sqlite",
            ".stateful/memory.sqlite-wal",
            ".stateful/memory.sqlite-shm",
            ".stateful/artifacts/private.md",
            ".stateful/backups/private.md",
            ".stateful/exports/private.md",
            "nested/.stateful/private.md",
        ] {
            let path = root.path().join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("directory");
            fs::write(path, "private_storage_canary").expect("private fixture");
        }
        for name in ["facts.md", ".ordinary", ".stateful-notes", "stateful.md"] {
            fs::write(root.path().join(name), "ordinary_source").expect("ordinary fixture");
        }
        if git {
            // Forced tracked private content must still be excluded.
            assert!(
                Command::new("git")
                    .args(["-C"])
                    .arg(root.path())
                    .args(["add", "-f", "."])
                    .output()
                    .expect("git add")
                    .status
                    .success()
            );
        }
        let scan = scan_roots_with_limits(
            &[root.path().to_path_buf()],
            ScanLimits {
                max_files: 4,
                max_project_regions: 100,
            },
            scan_file,
        )
        .expect("scan");
        let mut paths = scan
            .files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        assert_eq!(
            paths,
            vec![".ordinary", ".stateful-notes", "facts.md", "stateful.md"]
        );
        assert_eq!(
            (scan.inventory_complete, scan.truncated, scan.files_skipped),
            (true, false, 0)
        );
        for name in [".stateful", ".stateful/artifacts", "nested/.stateful"] {
            let scan = scan_roots(&[root.path().join(name)]).expect("private root scan");
            assert_eq!(
                (
                    scan.files.len(),
                    scan.inventory_complete,
                    scan.files_skipped
                ),
                (0, true, 0)
            );
        }
        for name in [".stateful/memory.sqlite", "nested/.stateful/private.md"] {
            assert!(
                scan_project_file(
                    root.path(),
                    &ProjectRelativePath::parse(name).expect("path")
                )
                .expect("targeted scan")
                .is_none()
            );
        }
    }
}

#[test]
fn case_variants_follow_host_rules_and_a_file_named_stateful_remains_source() {
    let root = TempDir::new().expect("project");
    fs::write(root.path().join(".ignore"), "!.STATEFUL/\n!.STATEFUL/**\n").expect("whitelist");
    fs::create_dir(root.path().join(".STATEFUL")).expect("directory");
    fs::write(root.path().join(".STATEFUL/private.md"), "private").expect("fixture");
    let scan = scan_roots(&[root.path().to_path_buf()]).expect("scan");
    assert_eq!(
        scan.files.len(),
        usize::from(!cfg!(any(windows, target_os = "macos")))
    );
    assert_eq!(
        scan_project_file(
            root.path(),
            &ProjectRelativePath::parse(".STATEFUL/private.md").expect("path")
        )
        .expect("targeted scan")
        .is_some(),
        !cfg!(any(windows, target_os = "macos"))
    );
    let leaf = TempDir::new().expect("project");
    fs::write(leaf.path().join(".ignore"), "!.stateful\n").expect("whitelist");
    fs::write(leaf.path().join(".stateful"), "ordinary file").expect("fixture");
    assert_eq!(
        scan_roots(&[leaf.path().to_path_buf()])
            .expect("scan")
            .files
            .len(),
        1
    );
    assert!(
        scan_project_file(
            leaf.path(),
            &ProjectRelativePath::parse(".stateful").expect("path")
        )
        .expect("targeted scan")
        .is_some()
    );
}

#[test]
fn aliases_into_private_storage_are_not_read() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    let root = TempDir::new().expect("project");
    let storage = root.path().join(".stateful");
    fs::create_dir(&storage).expect("storage");
    fs::write(storage.join("private.md"), "private").expect("fixture");
    #[cfg(unix)]
    symlink(&storage, root.path().join("alias")).expect("directory alias");
    #[cfg(windows)]
    {
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(root.path().join("alias"))
            .arg(&storage)
            .output()
            .expect("junction");
        assert!(output.status.success(), "junction failed: {output:?}");
    }
    #[cfg(unix)]
    symlink(storage.join("private.md"), root.path().join("alias.md")).expect("file alias");
    {
        let path = "alias/private.md";
        assert!(
            scan_project_file(
                root.path(),
                &ProjectRelativePath::parse(path).expect("path")
            )
            .expect("targeted scan")
            .is_none()
        );
    }
    #[cfg(unix)]
    assert!(
        scan_project_file(
            root.path(),
            &ProjectRelativePath::parse("alias.md").expect("path")
        )
        .expect("file alias scan")
        .is_none()
    );
    assert!(
        scan_roots(&[root.path().join("alias")])
            .expect("aliased root scan")
            .files
            .is_empty()
    );
}
