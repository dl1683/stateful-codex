#![expect(
    clippy::disallowed_methods,
    reason = "project-store tests own isolated SQLite connections without engine homes"
)]

use super::*;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::sqlite::SqliteConnectOptions;
use std::borrow::Cow;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn migrator(sql: &'static str) -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![
            Migration::new(
                1,
                "first".into(),
                MigrationType::Simple,
                sqlx::SqlStr::from_static(sql),
                /*no_tx*/ false,
            ),
            Migration::new(
                10,
                "gap".into(),
                MigrationType::Simple,
                sqlx::SqlStr::from_static("SELECT 1;"),
                /*no_tx*/ false,
            ),
        ]),
        ..Migrator::DEFAULT
    }
}

async fn prepared() -> Result<PreparedDomains, StoreRegistryError> {
    PreparedDomains::prepare(&[
        DomainManifest {
            domain: "alpha",
            migrations: &migrator(
                "CREATE TABLE alpha (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT);",
            ),
        },
        DomainManifest {
            domain: "bravo",
            migrations: &migrator("CREATE TABLE bravo (id INTEGER REFERENCES alpha(id));"),
        },
    ])
    .await
}

#[tokio::test]
async fn changed_partial_unknown_history_schema_and_format_refuse() -> TestResult {
    let domains = prepared().await?;
    for mutation in [
        "UPDATE project_store_migrations SET digest = zeroblob(32) WHERE domain = 'alpha'",
        "DELETE FROM project_store_migrations WHERE domain = 'bravo' AND version = 10",
        "INSERT INTO project_store_migrations VALUES ('unknown', 1, zeroblob(32))",
        "UPDATE project_store_migrations SET domain = CAST(zeroblob(1048576) AS TEXT) WHERE domain = 'alpha' AND version = 1",
        "CREATE TABLE unexpected(value TEXT)",
        "CREATE TRIGGER unexpected AFTER INSERT ON alpha BEGIN SELECT 1; END",
        "UPDATE project_store_identity SET format_version = 3",
        "UPDATE project_store_identity SET min_reader_version = 3",
        "UPDATE project_store_identity SET format_version = 1",
    ] {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        crate::initialize_connection(&mut connection).await?;
        domains.apply(&mut connection).await?;
        sqlx::raw_sql(mutation).execute(&mut connection).await?;
        let before = (
            schema(&mut connection).await?,
            history(&mut connection).await?,
        );
        assert!(
            domains.read_identity(&mut connection).await.is_err(),
            "{mutation}"
        );
        assert_eq!(
            (
                schema(&mut connection).await?,
                history(&mut connection).await?
            ),
            before
        );
        connection.close().await?;
    }
    let changed = PreparedDomains::prepare(&[DomainManifest {
        domain: "alpha",
        migrations: &migrator("CREATE TABLE changed(value TEXT);"),
    }])
    .await?;
    let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
    crate::initialize_connection(&mut connection).await?;
    domains.apply(&mut connection).await?;
    assert!(changed.read_identity(&mut connection).await.is_err());
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn preparation_refuses_invalid_manifests_and_kernel_changes() -> TestResult {
    assert!(PreparedDomains::prepare(&[]).await.is_err());
    for domain in ["", "project-store", "Bad", "a/b"] {
        assert!(
            PreparedDomains::prepare(&[DomainManifest {
                domain,
                migrations: &migrator("SELECT 1;")
            }])
            .await
            .is_err()
        );
    }
    for sql in [
        "ALTER TABLE project_store_identity ADD COLUMN bad TEXT;",
        "UPDATE project_store_identity SET project_uuid = '00000000-0000-0000-0000-000000000000';",
    ] {
        assert!(
            PreparedDomains::prepare(&[DomainManifest {
                domain: "alpha",
                migrations: &migrator(sql)
            }])
            .await
            .is_err()
        );
    }
    let list = migrator("SELECT 1;");
    assert!(
        PreparedDomains::prepare(&[
            DomainManifest {
                domain: "alpha",
                migrations: &list
            },
            DomainManifest {
                domain: "alpha",
                migrations: &list
            }
        ])
        .await
        .is_err()
    );
    for variation in 0..4 {
        let mut list = migrator("SELECT 1;");
        match variation {
            0 => list.no_tx = true,
            1 => list.migrations.to_mut()[1].version = 1,
            2 => list.migrations.to_mut()[1].no_tx = true,
            3 => list.migrations.to_mut()[1].migration_type = MigrationType::ReversibleDown,
            _ => unreachable!(),
        }
        assert!(
            PreparedDomains::prepare(&[DomainManifest {
                domain: "alpha",
                migrations: &list
            }])
            .await
            .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn complete_manifest_admits_read_only_and_preserves_bootstrap_history() -> TestResult {
    let directory = tempfile::tempdir()?;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(directory.path().join("memory.sqlite"))
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let identity = crate::initialize_connection(&mut connection).await?;
    let domains = prepared().await?;
    // Fixture construction only; the supported transactional installer is a later unit.
    domains.apply(&mut connection).await?;
    assert_eq!(domains.read_identity(&mut connection).await?, identity);
    assert!(crate::read_identity(&mut connection).await.is_err());
    let bootstrap: Vec<u8> = sqlx::query_scalar("SELECT digest FROM project_store_migrations WHERE domain = 'project-store' AND version = 1")
        .fetch_one(&mut connection).await?;
    assert_eq!(bootstrap, Sha256::digest(SCHEMA).to_vec());
    connection.close().await?;
    let mut reader =
        SqliteConnection::connect_with(&options.read_only(true).create_if_missing(false)).await?;
    assert_eq!(domains.read_identity(&mut reader).await?, identity);
    reader.close().await?;
    Ok(())
}

#[tokio::test]
async fn domains_preserve_identity_versions_and_read_only_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("memory.sqlite"))
        .create_if_missing(true)
        .busy_timeout(Duration::from_millis(/*millis*/ 50));
    let mut first = SqliteConnection::connect_with(&options).await?;
    let identity = crate::initialize_connection(&mut first).await?;
    let domains = prepared().await?;
    let mut second = SqliteConnection::connect_with(&options).await?;
    let (a, b) = tokio::join!(domains.install(&mut first), domains.install(&mut second));
    assert_eq!((a?, b?), (identity.clone(), identity.clone()));
    assert_eq!(domains.install(&mut first).await?, identity);
    assert!(crate::read_identity(&mut first).await.is_err());
    assert!(crate::initialize_connection(&mut first).await.is_err());
    let versions: Vec<(String, i64)> = sqlx::query_as(
        "SELECT domain, version FROM project_store_migrations ORDER BY domain, version",
    )
    .fetch_all(&mut first)
    .await?;
    assert_eq!(
        versions,
        vec![
            ("alpha".into(), 1),
            ("alpha".into(), 10),
            ("bravo".into(), 1),
            ("bravo".into(), 10),
            ("project-store".into(), 1),
            ("project-store".into(), 2)
        ]
    );
    first.close().await?;
    second.close().await?;
    let mut reader =
        SqliteConnection::connect_with(&options.read_only(true).create_if_missing(false)).await?;
    assert_eq!(domains.read_identity(&mut reader).await?, identity);
    reader.close().await?;
    Ok(())
}

#[tokio::test]
async fn failed_late_install_rolls_back_ddl_history_and_format() -> TestResult {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
    let identity = crate::initialize_connection(&mut connection).await?;
    let before = schema(&mut connection).await?;
    let domains = prepared().await?;
    sqlx::raw_sql("CREATE TEMP TRIGGER deny_bravo BEFORE INSERT ON main.project_store_migrations WHEN NEW.domain = 'bravo' BEGIN SELECT RAISE(ABORT, 'injected late failure'); END;")
        .execute(&mut connection).await?;
    assert!(domains.install(&mut connection).await.is_err());
    assert_eq!(schema(&mut connection).await?, before);
    assert_eq!(crate::read_identity(&mut connection).await?, identity);
    sqlx::query("DROP TRIGGER temp.deny_bravo")
        .execute(&mut connection)
        .await?;
    assert_eq!(domains.install(&mut connection).await?, identity);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn writer_busy_is_bounded_and_retry_preserves_identity() -> TestResult {
    let directory = tempfile::tempdir()?;
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("memory.sqlite"))
        .create_if_missing(true)
        .busy_timeout(Duration::from_millis(/*millis*/ 50));
    let mut first = SqliteConnection::connect_with(&options).await?;
    let mut second = SqliteConnection::connect_with(&options).await?;
    let identity = crate::initialize_connection(&mut first).await?;
    let domains = prepared().await?;
    let lock = first.begin_with("BEGIN IMMEDIATE").await?;
    match domains.install(&mut second).await {
        Err(StoreRegistryError::Sqlite(sqlx::Error::Database(error))) => {
            assert_eq!(error.code().as_deref(), Some("5"))
        }
        result => panic!("expected SQLITE_BUSY, got {result:?}"),
    }
    lock.rollback().await?;
    assert_eq!(domains.install(&mut second).await?, identity);
    first.close().await?;
    second.close().await?;
    Ok(())
}
