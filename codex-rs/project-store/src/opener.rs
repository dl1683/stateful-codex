use std::fs;
use std::io;
use std::io::Write;
use std::time::Duration;

use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use thiserror::Error;
use uuid::Uuid;

use crate::FilesystemQualificationError;
use crate::ProjectStoreLocation;
use crate::ResolveError;
use crate::StoreIdentity;
use crate::StoreRegistryError;
use crate::initialize_connection;
use crate::qualify_local_filesystem;
use crate::read_identity;
use codex_utils_absolute_path::AbsolutePathBuf;

const IGNORE: &[u8] = b"*\n!.gitignore\n";

/// Initialize only the kernel at a host-qualified local unsynchronized root.
///
/// Native writers remain unattached. The caller must establish absence of sync
/// software beyond the conservative filesystem checks here. Existing stores are
/// inspected read-only. A complete, closed staging database is published without
/// replacing any existing file; racing initializers return the winning identity.
/// Killed initializers may leave ignored inactive staging files, never a partial
/// canonical database. Power-loss durability is not qualified by this operation.
pub async fn initialize_local_store(
    location: &ProjectStoreLocation,
) -> Result<StoreIdentity, ProjectStoreOpenError> {
    initialize_qualified(location, &qualify_for_open).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "project-store owns its SQLite connections independently of codex-state"
)]
async fn initialize_qualified(
    location: &ProjectStoreLocation,
    qualification: &impl Fn(&AbsolutePathBuf) -> Result<(), ProjectStoreOpenError>,
) -> Result<StoreIdentity, ProjectStoreOpenError> {
    revalidate_location(location)?;
    qualification(location.project_root())?;
    if location.database_path().as_path().try_exists()? {
        return inspect_qualified(location, qualification).await;
    }
    let directory = location.project_root().join(".stateful");
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(/*mode*/ 0o700);
    }
    match builder.create(directory.as_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    revalidate_location(location)?;
    qualification(&directory)?;
    let ignore = directory.join(".gitignore");
    let temporary_ignore = directory.join(format!(".ignore-{}", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary_ignore.as_path())?;
    file.write_all(IGNORE)?;
    drop(file);
    let published_ignore = fs::hard_link(temporary_ignore.as_path(), ignore.as_path());
    fs::remove_file(temporary_ignore.as_path())?;
    match published_ignore {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(ignore.as_path())?;
    if !plain_file(&metadata) || fs::read(ignore.as_path())? != IGNORE {
        return Err(ProjectStoreOpenError::IgnorePolicy);
    }
    let staging = directory.join(format!(".bootstrap-{}.sqlite", Uuid::new_v4()));
    // Reserve the name without overwriting a preexisting file.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.as_path())?;
    let options = SqliteConnectOptions::new()
        .filename(staging.as_path())
        .foreign_keys(/*on*/ true)
        .busy_timeout(Duration::from_secs(/*secs*/ 2))
        // A closed DELETE-mode staging file has no required WAL sidecar.
        // Native WAL pools are a separate, gated attachment step.
        .journal_mode(SqliteJournalMode::Delete);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = initialize_connection(&mut connection).await;
    connection.close().await?;
    result?;
    let publication = fs::hard_link(staging.as_path(), location.database_path().as_path());
    fs::remove_file(staging.as_path())?;
    match publication {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    inspect_qualified(location, qualification).await
}

/// Read identity under a consistent snapshot without writable SQLite hooks.
/// SQLite may create operational sidecars when reading an existing WAL database.
pub async fn inspect_store(
    location: &ProjectStoreLocation,
) -> Result<StoreIdentity, ProjectStoreOpenError> {
    inspect_qualified(location, &qualify_for_open).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "project-store owns read-only SQLite inspection independently of codex-state"
)]
async fn inspect_qualified(
    location: &ProjectStoreLocation,
    qualification: &impl Fn(&AbsolutePathBuf) -> Result<(), ProjectStoreOpenError>,
) -> Result<StoreIdentity, ProjectStoreOpenError> {
    qualify_store(location, qualification)?;
    let options = SqliteConnectOptions::new()
        .filename(location.database_path().as_path())
        .read_only(true)
        .busy_timeout(Duration::from_secs(/*secs*/ 2));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = read_identity(&mut connection).await;
    connection.close().await?;
    Ok(result?)
}

pub(crate) fn qualify_store(
    location: &ProjectStoreLocation,
    qualification: &impl Fn(&AbsolutePathBuf) -> Result<(), ProjectStoreOpenError>,
) -> Result<(), ProjectStoreOpenError> {
    revalidate_location(location)?;
    qualification(location.project_root())?;
    qualification(&location.project_root().join(".stateful"))?;
    qualification(location.database_path())?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = location
            .project_root()
            .join(format!(".stateful/memory.sqlite{suffix}"));
        let metadata = match fs::symlink_metadata(sidecar.as_path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !plain_file(&metadata) {
            return Err(ResolveError::UnsafeStorePath.into());
        }
        qualification(&sidecar)?;
    }
    Ok(())
}

pub(crate) fn qualify_for_open(path: &AbsolutePathBuf) -> Result<(), ProjectStoreOpenError> {
    qualify_local_filesystem(path).map_err(ProjectStoreOpenError::from)
}

fn plain_file(metadata: &fs::Metadata) -> bool {
    let redirected = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let redirected = {
        use std::os::windows::fs::MetadataExt;
        redirected || metadata.file_attributes() & 0x400 != 0
    };
    metadata.is_file() && !redirected
}

impl From<FilesystemQualificationError> for ProjectStoreOpenError {
    fn from(error: FilesystemQualificationError) -> Self {
        match error {
            FilesystemQualificationError::UnsupportedFilesystem => Self::UnsupportedFilesystem,
            FilesystemQualificationError::Io(error) => Self::Io(error),
        }
    }
}

fn revalidate_location(location: &ProjectStoreLocation) -> Result<(), ProjectStoreOpenError> {
    if ProjectStoreLocation::select_project(location.project_root())? != *location {
        return Err(ProjectStoreOpenError::LocationChanged);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ProjectStoreOpenError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
    #[error(transparent)]
    Registry(#[from] StoreRegistryError),
    #[error("the project requires a qualified local, unsynchronized filesystem")]
    UnsupportedFilesystem,
    #[error("the selected project-store location changed")]
    LocationChanged,
    #[error("existing .stateful/.gitignore differs from the required private-store policy")]
    IgnorePolicy,
}

#[cfg(test)]
#[path = "opener_tests.rs"]
mod tests;
