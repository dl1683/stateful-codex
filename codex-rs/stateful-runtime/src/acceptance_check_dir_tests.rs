use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::CwdReason;
use super::stored_check_directory;

#[test]
fn absolute_and_relative_directories_have_one_stored_form() {
    let root = TempDir::new().expect("root");
    let root = root.path().canonicalize().expect("canonical root");
    let directory = root.join("real/nested");
    std::fs::create_dir_all(&directory).expect("directory");
    for input in [
        "real/./nested".to_string(),
        directory.to_str().expect("UTF-8").to_string(),
    ] {
        let stored = stored_check_directory(Some(&root), &input).expect("contained");
        assert_eq!(
            (stored.text, stored.directory),
            ("real/nested".to_string(), directory.clone())
        );
    }
    let stored = stored_check_directory(Some(&root), ".").expect("root");
    assert_eq!((stored.text.as_str(), &stored.directory), (".", &root));
}

#[test]
fn invalid_or_second_root_directories_are_refused() {
    let root = TempDir::new().expect("first root");
    let second = TempDir::new().expect("second root");
    std::fs::write(root.path().join("file"), "not a directory").expect("file");
    for input in [
        "",
        "missing",
        "file",
        "../escape",
        "real/../other",
        "bad\nname",
    ] {
        assert_eq!(
            stored_check_directory(Some(root.path()), input).err(),
            Some(CwdReason::InvalidDirectory),
            "{input:?}"
        );
    }
    assert_eq!(
        stored_check_directory(Some(root.path()), second.path().to_str().expect("UTF-8")).err(),
        Some(CwdReason::OutsideRoot)
    );
    assert_eq!(
        stored_check_directory(/*project_root*/ None, ".").err(),
        Some(CwdReason::NeedsRoot)
    );
}

// Windows uses an unprivileged junction; Unix exercises the same boundary with a symlink.
#[test]
fn directory_aliases_are_canonicalized_and_escaping_aliases_are_refused() {
    let root = TempDir::new().expect("root");
    let outside = TempDir::new().expect("outside");
    std::fs::create_dir(root.path().join("real")).expect("real");
    directory_alias(&root.path().join("real"), &root.path().join("alias"));
    directory_alias(outside.path(), &root.path().join("escape"));
    let stored = stored_check_directory(Some(root.path()), "alias").expect("alias inside");
    assert_eq!(stored.text, "real");
    assert_eq!(
        stored_check_directory(Some(root.path()), "escape").err(),
        Some(CwdReason::OutsideRoot)
    );
}

fn directory_alias(target: &std::path::Path, alias: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, alias).expect("directory symlink");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(alias)
            .arg(target)
            .output()
            .expect("junction preflight starts");
        assert!(output.status.success(), "junction preflight: {output:?}");
    }
    assert_eq!(
        alias.canonicalize().expect("alias resolves"),
        target.canonicalize().expect("target")
    );
}
