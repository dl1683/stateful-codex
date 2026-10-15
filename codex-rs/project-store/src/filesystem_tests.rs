use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn existing_directory_and_file_can_be_qualified_without_initializing_state() {
    let project = TempDir::new().unwrap();
    let file = project.path().join("input");
    fs::write(&file, b"source").unwrap();
    for target in [project.path(), file.as_path()] {
        let path = AbsolutePathBuf::from_absolute_path(target).unwrap();
        qualify_local_filesystem(&path).unwrap();
    }
    assert!(!project.path().join(".stateful").exists());
}

#[test]
fn recognized_sync_directory_is_not_local_store_admission() {
    let project = TempDir::new().unwrap();
    let synced = project.path().join("OneDrive");
    fs::create_dir(&synced).unwrap();
    let path = AbsolutePathBuf::from_absolute_path(&synced).unwrap();
    assert!(matches!(
        qualify_local_filesystem(&path),
        Err(FilesystemQualificationError::UnsupportedFilesystem)
    ));
    assert!(!synced.join(".stateful").exists());
}
