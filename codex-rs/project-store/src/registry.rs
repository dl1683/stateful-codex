use sha2::Digest;
use sha2::Sha256;
use sqlx::Connection;
use sqlx::SqliteConnection;
use thiserror::Error;
use uuid::Uuid;

pub(crate) const SCHEMA: &str = include_str!("../migrations/0001_bootstrap.sql");
const VERSION: i64 = 1;

#[derive(sqlx::FromRow)]
struct StoredIdentity {
    project_uuid: Option<String>,
    store_uuid: Option<String>,
    format_version: i64,
    min_reader_version: i64,
    min_writer_version: i64,
}

/// Stable project and store identities. Moving a store preserves both; copying
/// does not authorize an independent writable fork of these identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreIdentity {
    pub project_uuid: Uuid,
    pub store_uuid: Uuid,
}

/// Initialize fixed bootstrap metadata, or validate an existing bootstrap store.
///
/// The caller owns placement/filesystem qualification and must configure a bounded
/// busy timeout before calling. The immediate transaction serializes independent
/// connections; no lock is held across any caller work. This inert foundation
/// supports only bootstrap format 1, not imported application-domain histories.
pub async fn initialize_connection(
    connection: &mut SqliteConnection,
) -> Result<StoreIdentity, StoreRegistryError> {
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let objects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'")
            .fetch_one(&mut *transaction)
            .await?;
    validate_header(&mut transaction).await?;
    if objects == 0 {
        sqlx::raw_sql(SCHEMA).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO project_store_identity VALUES (1, ?, ?, ?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(Uuid::new_v4().to_string())
            .bind(VERSION)
            .bind(VERSION)
            .bind(VERSION)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO project_store_migrations VALUES ('project-store', 1, ?)")
            .bind(Sha256::digest(SCHEMA.as_bytes()).to_vec())
            .execute(&mut *transaction)
            .await?;
    }
    let identity = validate(&mut transaction).await?;
    transaction.commit().await?;
    Ok(identity)
}

/// Validate metadata and registry under a consistent read snapshot. This issues
/// no write statements and works with a read-only SQLite connection.
pub async fn read_identity(
    connection: &mut SqliteConnection,
) -> Result<StoreIdentity, StoreRegistryError> {
    let mut transaction = connection.begin().await?;
    let identity = validate(&mut transaction).await?;
    transaction.commit().await?;
    Ok(identity)
}

pub(crate) async fn validate_header(
    connection: &mut SqliteConnection,
) -> Result<(), StoreRegistryError> {
    let application_id: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&mut *connection)
        .await?;
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *connection)
        .await?;
    if application_id != 0 || user_version != 0 {
        return Err(StoreRegistryError::UnknownSchema);
    }
    Ok(())
}

pub(crate) async fn validate(
    connection: &mut SqliteConnection,
) -> Result<StoreIdentity, StoreRegistryError> {
    validate_header(connection).await?;
    let actual: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT CASE WHEN octet_length(sql) <= 4096 THEN sql END
         FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name LIMIT 3",
    )
    .fetch_all(&mut *connection)
    .await?;
    let expected = SCHEMA
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(Some);
    if !actual.iter().map(Option::as_deref).eq(expected) {
        return Err(StoreRegistryError::UnknownSchema);
    }
    let identity = validate_identity(connection, VERSION).await?;
    let history: Vec<(Option<String>, i64, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT CASE WHEN octet_length(domain) <= 64 THEN domain END, version,
                CASE WHEN octet_length(digest) = 32 THEN digest END
         FROM project_store_migrations ORDER BY domain, version LIMIT 2",
    )
    .fetch_all(&mut *connection)
    .await?;
    if history
        != vec![(
            Some("project-store".to_string()),
            VERSION,
            Some(Sha256::digest(SCHEMA.as_bytes()).to_vec()),
        )]
    {
        return Err(StoreRegistryError::IncompatibleHistory);
    }
    Ok(identity)
}

pub(crate) async fn validate_identity(
    connection: &mut SqliteConnection,
    version: i64,
) -> Result<StoreIdentity, StoreRegistryError> {
    let identities: Vec<StoredIdentity> = sqlx::query_as(
        "SELECT CASE WHEN octet_length(project_uuid) = 36 THEN project_uuid END AS project_uuid,
                CASE WHEN octet_length(store_uuid) = 36 THEN store_uuid END AS store_uuid,
                format_version, min_reader_version, min_writer_version
         FROM project_store_identity
         ORDER BY singleton LIMIT 2",
    )
    .fetch_all(&mut *connection)
    .await?;
    let [
        StoredIdentity {
            project_uuid: Some(project),
            store_uuid: Some(store),
            format_version: format,
            min_reader_version: reader,
            min_writer_version: writer,
        },
    ] = identities.as_slice()
    else {
        return Err(StoreRegistryError::InvalidIdentity);
    };
    if (*format, *reader, *writer) != (version, version, version) {
        return Err(StoreRegistryError::UnsupportedFormat);
    }
    let project_uuid = Uuid::parse_str(project).map_err(|_| StoreRegistryError::InvalidIdentity)?;
    let store_uuid = Uuid::parse_str(store).map_err(|_| StoreRegistryError::InvalidIdentity)?;
    if project_uuid.is_nil()
        || store_uuid.is_nil()
        || project_uuid.to_string() != *project
        || store_uuid.to_string() != *store
    {
        return Err(StoreRegistryError::InvalidIdentity);
    }
    Ok(StoreIdentity {
        project_uuid,
        store_uuid,
    })
}

#[derive(Debug, Error)]
pub enum StoreRegistryError {
    #[error("unsupported or partial project-store schema; no automatic repair was attempted")]
    UnknownSchema,
    #[error("missing or invalid project/store identity")]
    InvalidIdentity,
    #[error("unsupported project-store format or reader/writer version")]
    UnsupportedFormat,
    #[error("unknown or changed project-store migration history")]
    IncompatibleHistory,
    #[error(transparent)]
    Sqlite(#[from] sqlx::Error),
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
