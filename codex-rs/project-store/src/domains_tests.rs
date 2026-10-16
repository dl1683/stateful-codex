#![expect(
    clippy::disallowed_methods,
    reason = "project-store tests own isolated SQLite connections without engine homes"
)]

use super::*;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use std::borrow::Cow;

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
