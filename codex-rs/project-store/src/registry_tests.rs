use super::*;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn connection(directory: &Path) -> TestResult<SqliteConnection> {
    let config = SqliteConfig::new_for_testing(AbsolutePathBuf::from_absolute_path(directory)?);
    let pool = config
        .open_read_write_pool(&directory.join("memory.sqlite"))
        .await?;
    let connection = pool.acquire().await?.detach();
    pool.close().await;
    Ok(connection)
}

#[tokio::test]
async fn independent_initializers_and_read_only_reopen_preserve_identity() -> TestResult {
    let directory = TempDir::new()?;
    let mut first = connection(directory.path()).await?;
    let mut second = connection(directory.path()).await?;
    let (a, b) = tokio::join!(
        initialize_connection(&mut first),
        initialize_connection(&mut second)
    );
    let identity = a?;
    assert_eq!(identity, b?);
    assert_eq!(identity, initialize_connection(&mut first).await?);
    first.close().await?;
    second.close().await?;
    let config =
        SqliteConfig::new_for_testing(AbsolutePathBuf::from_absolute_path(directory.path())?);
    let pool = config
        .open_read_only_pool(
            &directory.path().join("memory.sqlite"),
            Some(Duration::from_secs(3)),
        )
        .await?;
    let mut read_only = pool.acquire().await?.detach();
    pool.close().await;
    assert_eq!(identity, read_identity(&mut read_only).await?);
    read_only.close().await?;
    Ok(())
}

#[tokio::test]
async fn different_stores_receive_distinct_identities() -> TestResult {
    let first_directory = TempDir::new()?;
    let second_directory = TempDir::new()?;
    let mut first = connection(first_directory.path()).await?;
    let mut second = connection(second_directory.path()).await?;
    let a = initialize_connection(&mut first).await?;
    let b = initialize_connection(&mut second).await?;
    assert_ne!(a.project_uuid, b.project_uuid);
    assert_ne!(a.store_uuid, b.store_uuid);
    first.close().await?;
    second.close().await?;
    Ok(())
}

#[tokio::test]
async fn foreign_schema_and_partial_bootstrap_refuse_without_repair() -> TestResult {
    let directory = TempDir::new()?;
    let mut connection = connection(directory.path()).await?;
    sqlx::query("CREATE TABLE owner_canary (value TEXT)")
        .execute(&mut connection)
        .await?;
    assert!(matches!(
        initialize_connection(&mut connection).await,
        Err(StoreRegistryError::UnknownSchema)
    ));
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema ORDER BY name")
        .fetch_all(&mut connection)
        .await?;
    assert_eq!(tables, vec!["owner_canary".to_string()]);
    sqlx::query("DROP TABLE owner_canary")
        .execute(&mut connection)
        .await?;
    sqlx::raw_sql(SCHEMA).execute(&mut connection).await?;
    assert!(matches!(
        initialize_connection(&mut connection).await,
        Err(StoreRegistryError::InvalidIdentity)
    ));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM project_store_identity")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(rows, 0);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn changed_ledger_format_and_uuid_are_not_silently_repaired() -> TestResult {
    for (change, expected) in [
        (
            "UPDATE project_store_migrations SET digest = zeroblob(32)",
            "history",
        ),
        ("DELETE FROM project_store_migrations", "history"),
        (
            "INSERT INTO project_store_migrations VALUES ('unknown', 1, zeroblob(32))",
            "history",
        ),
        (
            "UPDATE project_store_identity SET format_version = 2",
            "format",
        ),
        (
            "UPDATE project_store_identity SET min_reader_version = 2",
            "format",
        ),
        (
            "UPDATE project_store_identity SET project_uuid = 'not-a-uuid'",
            "identity",
        ),
        (
            "UPDATE project_store_identity SET store_uuid = '00000000-0000-0000-0000-000000000000'",
            "identity",
        ),
    ] {
        let directory = TempDir::new()?;
        let mut connection = connection(directory.path()).await?;
        initialize_connection(&mut connection).await?;
        sqlx::query(change).execute(&mut connection).await?;
        let error = initialize_connection(&mut connection).await.unwrap_err();
        match (expected, error) {
            ("history", StoreRegistryError::IncompatibleHistory)
            | ("format", StoreRegistryError::UnsupportedFormat)
            | ("identity", StoreRegistryError::InvalidIdentity) => {}
            (expected, error) => panic!("expected {expected}, received {error}"),
        }
        connection.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn failed_bootstrap_rolls_back_schema_and_can_retry() -> TestResult {
    let directory = TempDir::new()?;
    let mut connection = connection(directory.path()).await?;
    sqlx::query("PRAGMA max_page_count = 2")
        .execute(&mut connection)
        .await?;
    assert!(matches!(
        initialize_connection(&mut connection).await,
        Err(StoreRegistryError::Sqlite(_))
    ));
    let objects: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_schema")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(objects, 0);
    sqlx::query("PRAGMA max_page_count = 100")
        .execute(&mut connection)
        .await?;
    let identity = initialize_connection(&mut connection).await?;
    assert_eq!(identity, read_identity(&mut connection).await?);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_like_user_table_is_foreign_schema() -> TestResult {
    let directory = TempDir::new()?;
    let mut connection = connection(directory.path()).await?;
    sqlx::query("CREATE TABLE sqliteXforeign (value TEXT)")
        .execute(&mut connection)
        .await?;
    assert!(matches!(
        initialize_connection(&mut connection).await,
        Err(StoreRegistryError::UnknownSchema)
    ));
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn foreign_header_without_tables_is_not_an_empty_store() -> TestResult {
    for pragma in ["PRAGMA application_id = 12345", "PRAGMA user_version = 99"] {
        for initialize in [false, true] {
            let directory = TempDir::new()?;
            let mut connection = connection(directory.path()).await?;
            if initialize {
                initialize_connection(&mut connection).await?;
            }
            let before: Vec<String> =
                sqlx::query_scalar("SELECT name FROM sqlite_schema ORDER BY name")
                    .fetch_all(&mut connection)
                    .await?;
            sqlx::query(pragma).execute(&mut connection).await?;
            assert!(matches!(
                initialize_connection(&mut connection).await,
                Err(StoreRegistryError::UnknownSchema)
            ));
            if initialize {
                assert!(matches!(
                    read_identity(&mut connection).await,
                    Err(StoreRegistryError::UnknownSchema)
                ));
            }
            let objects: Vec<String> =
                sqlx::query_scalar("SELECT name FROM sqlite_schema ORDER BY name")
                    .fetch_all(&mut connection)
                    .await?;
            assert_eq!(objects, before);
            connection.close().await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn oversized_history_fields_refuse_with_bounded_projection() -> TestResult {
    for change in [
        "UPDATE project_store_migrations SET domain = CAST(zeroblob(1048576) AS TEXT)",
        "UPDATE project_store_migrations SET digest = zeroblob(1048576)",
    ] {
        let directory = TempDir::new()?;
        let mut connection = connection(directory.path()).await?;
        initialize_connection(&mut connection).await?;
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut connection)
            .await?;
        sqlx::query(change).execute(&mut connection).await?;
        assert!(matches!(
            read_identity(&mut connection).await,
            Err(StoreRegistryError::IncompatibleHistory)
        ));
        connection.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn malformed_extra_identity_is_not_filtered_out() -> TestResult {
    let directory = TempDir::new()?;
    let mut connection = connection(directory.path()).await?;
    initialize_connection(&mut connection).await?;
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut connection)
        .await?;
    sqlx::query("INSERT INTO project_store_identity VALUES (2, 'x', 'y', 1, 1, 1)")
        .execute(&mut connection)
        .await?;
    assert!(matches!(
        read_identity(&mut connection).await,
        Err(StoreRegistryError::InvalidIdentity)
    ));
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn writer_contention_refuses_then_recovers_without_identity_changes() -> TestResult {
    let directory = TempDir::new()?;
    let mut owner = connection(directory.path()).await?;
    let identity = initialize_connection(&mut owner).await?;
    let mut contender = connection(directory.path()).await?;
    sqlx::query("PRAGMA busy_timeout = 50")
        .execute(&mut contender)
        .await?;
    let lock = owner.begin_with("BEGIN IMMEDIATE").await?;
    match initialize_connection(&mut contender).await {
        Err(StoreRegistryError::Sqlite(sqlx::Error::Database(error))) => {
            assert_eq!(error.code().as_deref(), Some("5"));
        }
        result => panic!("expected SQLITE_BUSY, got {result:?}"),
    }
    lock.rollback().await?;
    assert_eq!(identity, initialize_connection(&mut contender).await?);
    owner.close().await?;
    contender.close().await?;
    Ok(())
}
