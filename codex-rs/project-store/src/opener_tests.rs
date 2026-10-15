use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn location(path: &std::path::Path) -> ProjectStoreLocation {
    ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(path).unwrap())
        .unwrap()
}

#[tokio::test]
async fn project_local_identity_survives_reopen_and_move_with_independent_projects() {
    let workspace = TempDir::new().unwrap();
    let first = workspace.path().join("first");
    let second = workspace.path().join("second");
    let moved = workspace.path().join("moved");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let first_identity = initialize_local_store(&location(&first)).await.unwrap();
    let second_identity = initialize_local_store(&location(&second)).await.unwrap();
    assert_ne!(first_identity, second_identity);
    assert_eq!(
        inspect_store(&location(&first)).await.unwrap(),
        first_identity
    );
    assert_eq!(
        initialize_local_store(&location(&first)).await.unwrap(),
        first_identity
    );
    fs::rename(&first, &moved).unwrap();
    assert_eq!(
        inspect_store(&location(&moved)).await.unwrap(),
        first_identity
    );
    assert_eq!(
        inspect_store(&location(&second)).await.unwrap(),
        second_identity
    );
}

#[tokio::test]
async fn racing_initializers_publish_one_complete_identity() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    let (first, second) =
        tokio::join!(initialize_local_store(&root), initialize_local_store(&root));
    let first = first.unwrap();
    assert_eq!(second.unwrap(), first);
    assert_eq!(inspect_store(&root).await.unwrap(), first);
    let mut files = fs::read_dir(project.path().join(".stateful"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(
        files,
        [".gitignore", "memory.sqlite"].map(std::ffi::OsString::from)
    );
}

#[tokio::test]
async fn unknown_existing_database_is_refused_without_byte_changes() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    fs::create_dir(project.path().join(".stateful")).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(root.database_path().as_path())
        .create_if_missing(/*create*/ true);
    #[expect(
        clippy::disallowed_methods,
        reason = "synthetic foreign database fixture"
    )]
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql("CREATE TABLE canary(value TEXT); INSERT INTO canary VALUES ('unrelated');")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let before = fs::read(root.database_path().as_path()).unwrap();
    assert!(matches!(
        initialize_local_store(&root).await,
        Err(ProjectStoreOpenError::Registry(
            StoreRegistryError::UnknownSchema
        ))
    ));
    assert_eq!(fs::read(root.database_path().as_path()).unwrap(), before);
}

#[tokio::test]
async fn empty_existing_file_is_not_an_initializer_reservation() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    fs::create_dir(project.path().join(".stateful")).unwrap();
    fs::write(root.database_path().as_path(), b"").unwrap();
    assert!(matches!(
        initialize_local_store(&root).await,
        Err(ProjectStoreOpenError::Registry(
            StoreRegistryError::UnknownSchema
        ))
    ));
    assert_eq!(fs::read(root.database_path().as_path()).unwrap(), b"");
}

#[tokio::test]
async fn missing_read_and_conflicting_ignore_policy_never_create_a_database() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    assert!(matches!(
        inspect_store(&root).await,
        Err(ProjectStoreOpenError::Io(error)) if error.kind() == io::ErrorKind::NotFound
    ));
    assert!(!project.path().join(".stateful").exists());
    fs::create_dir(project.path().join(".stateful")).unwrap();
    let ignore = project.path().join(".stateful/.gitignore");
    fs::write(&ignore, b"owner policy\n").unwrap();
    assert!(matches!(
        initialize_local_store(&root).await,
        Err(ProjectStoreOpenError::IgnorePolicy)
    ));
    assert_eq!(fs::read(ignore).unwrap(), b"owner policy\n");
    assert!(!root.database_path().as_path().exists());
}

#[tokio::test]
async fn inactive_staging_file_is_never_discovered_as_authority() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    fs::create_dir(project.path().join(".stateful")).unwrap();
    let orphan = project.path().join(".stateful/.bootstrap-abandoned.sqlite");
    fs::write(&orphan, b"incomplete inactive stage").unwrap();
    let identity = initialize_local_store(&root).await.unwrap();
    assert_eq!(inspect_store(&root).await.unwrap(), identity);
    assert_eq!(fs::read(orphan).unwrap(), b"incomplete inactive stage");
}

#[tokio::test]
async fn recognized_sync_directory_refuses_before_creating_state() {
    let workspace = TempDir::new().unwrap();
    let synced = workspace.path().join("OneDrive");
    fs::create_dir(&synced).unwrap();
    assert!(matches!(
        initialize_local_store(&location(&synced)).await,
        Err(ProjectStoreOpenError::UnsupportedFilesystem)
    ));
    assert!(!synced.join(".stateful").exists());
}

#[tokio::test]
async fn stale_location_refuses_before_creating_state_in_redirected_project() {
    let workspace = TempDir::new().unwrap();
    let original = workspace.path().join("original");
    let moved = workspace.path().join("moved");
    let other = TempDir::new().unwrap();
    fs::create_dir(&original).unwrap();
    let selected = location(&original);
    fs::rename(&original, &moved).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(other.path(), &original).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&original)
            .arg(other.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "junction setup failed: {output:?}");
    }
    assert!(matches!(
        initialize_local_store(&selected).await,
        Err(ProjectStoreOpenError::LocationChanged)
    ));
    assert!(!other.path().join(".stateful").exists());
    assert!(!moved.join(".stateful").exists());
}

#[tokio::test]
async fn stable_unsupported_store_directory_refuses_before_any_file_write() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    let store_directory = root.project_root().join(".stateful");
    fs::create_dir(store_directory.as_path()).unwrap();
    let qualification = |path: &AbsolutePathBuf| {
        if path == &store_directory {
            Err(ProjectStoreOpenError::UnsupportedFilesystem)
        } else {
            qualify_for_open(path)
        }
    };
    assert!(matches!(initialize_qualified(&root, &qualification).await, Err(ProjectStoreOpenError::UnsupportedFilesystem)));
    assert_eq!(fs::read_dir(store_directory.as_path()).unwrap().count(), 0);
}

#[tokio::test]
async fn stable_unsupported_database_mount_refuses_before_sqlite_inspection() {
    let project = TempDir::new().unwrap();
    let root = location(project.path());
    initialize_local_store(&root).await.unwrap();
    let before = fs::read(root.database_path().as_path()).unwrap();
    let qualification = |path: &AbsolutePathBuf| {
        if path == root.database_path() {
            Err(ProjectStoreOpenError::UnsupportedFilesystem)
        } else {
            qualify_for_open(path)
        }
    };
    assert!(matches!(inspect_qualified(&root, &qualification).await, Err(ProjectStoreOpenError::UnsupportedFilesystem)));
    assert_eq!(fs::read(root.database_path().as_path()).unwrap(), before);
}
