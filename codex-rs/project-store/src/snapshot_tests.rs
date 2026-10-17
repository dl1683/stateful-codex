#![expect(
    clippy::disallowed_methods,
    reason = "isolated project-store snapshot fixtures"
)]

use super::*;
use crate::PreparedDomains;
use crate::ProjectStoreLocation;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::sqlite::SqliteJournalMode;
use std::borrow::Cow;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    directory: tempfile::TempDir,
    location: ProjectStoreLocation,
    identity: StoreIdentity,
    writer: SqliteConnection,
    reader: ProjectDomainReader,
}

fn migrations() -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            1,
            "snapshot".into(),
            MigrationType::Simple,
            sqlx::SqlStr::from_static(
                "CREATE TABLE snapshot_values(value TEXT NOT NULL); INSERT INTO snapshot_values VALUES ('before');",
            ),
            /*no_tx*/ false,
        )]),
        ..Migrator::DEFAULT
    }
}

async fn fixture() -> TestResult<Fixture> {
    let directory = tempfile::tempdir()?;
    let location = ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(
        directory.path(),
    )?)?;
    let identity = crate::initialize_local_store(&location).await?;
    let migrations = migrations();
    let manifest = DomainManifest {
        domain: "snapshot",
        migrations: &migrations,
    };
    let domains = PreparedDomains::prepare(&[manifest]).await?;
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .journal_mode(SqliteJournalMode::Wal),
    )
    .await?;
    assert_eq!(domains.install(&mut writer).await?, identity);
    let reader = ProjectStoreReader::open(&location, domains)
        .await?
        .into_domain_reader(&DomainManifest {
            domain: "snapshot",
            migrations: &migrations,
        })
        .await?;
    Ok(Fixture {
        directory,
        location,
        identity,
        writer,
        reader,
    })
}

#[tokio::test]
async fn snapshot_requires_exact_compiled_domain_history() -> TestResult {
    let mut fixture = fixture().await?;
    let snapshot = fixture.reader.begin_snapshot().await?;
    let compiled = migrations();
    snapshot.require_domain(&DomainManifest {
        domain: "snapshot",
        migrations: &compiled,
    })?;
    let mut changed = migrations();
    changed.migrations.to_mut()[0].sql = sqlx::SqlStr::from_static("SELECT 1;");
    for manifest in [
        DomainManifest {
            domain: "other",
            migrations: &compiled,
        },
        DomainManifest {
            domain: "snapshot",
            migrations: &changed,
        },
    ] {
        assert!(matches!(
            snapshot.require_domain(&manifest),
            Err(ProjectReadError::Registry(
                StoreRegistryError::IncompatibleHistory
            ))
        ));
    }
    snapshot.finish().await?;
    fixture.reader.begin_snapshot().await?.finish().await?;
    fixture.reader.close().await?;
    fixture.writer.close().await?;
    Ok(())
}

#[tokio::test]
async fn writer_commit_is_visible_only_to_the_next_admitted_snapshot() -> TestResult {
    let mut fixture = fixture().await?;
    let mut snapshot = fixture.reader.begin_snapshot().await?;
    assert_eq!(snapshot.identity(), &fixture.identity);
    sqlx::query("UPDATE snapshot_values SET value = 'after'")
        .execute(&mut fixture.writer)
        .await?;
    let before: String = sqlx::query_scalar("SELECT value FROM snapshot_values")
        .fetch_one(snapshot.connection())
        .await?;
    assert_eq!(before, "before");
    snapshot.finish().await?;
    let mut snapshot = fixture.reader.begin_snapshot().await?;
    let after: String = sqlx::query_scalar("SELECT value FROM snapshot_values")
        .fetch_one(snapshot.connection())
        .await?;
    assert_eq!(
        (snapshot.identity(), after.as_str()),
        (&fixture.identity, "after")
    );
    match sqlx::query("DELETE FROM snapshot_values")
        .execute(snapshot.connection())
        .await
    {
        Err(sqlx::Error::Database(error)) => assert_eq!(error.code().as_deref(), Some("8")),
        result => panic!("expected readonly8, got {result:?}"),
    }
    snapshot.finish().await?;
    fixture.reader.close().await?;
    fixture.writer.close().await?;
    Ok(())
}

#[tokio::test]
async fn changed_identity_schema_history_or_header_refuses_before_domain_access() -> TestResult {
    for mutation in [
        "UPDATE project_store_identity SET project_uuid = '00000000-0000-4000-8000-000000000001'",
        "UPDATE project_store_identity SET store_uuid = '00000000-0000-4000-8000-000000000001'",
        "CREATE TABLE unexpected(value TEXT)",
        "UPDATE project_store_migrations SET digest = zeroblob(32) WHERE domain = 'snapshot'",
        "PRAGMA user_version = 9",
    ] {
        let mut fixture = fixture().await?;
        sqlx::query(mutation).execute(&mut fixture.writer).await?;
        let before = std::fs::read(fixture.location.database_path().as_path())?;
        assert!(matches!(
            fixture.reader.begin_snapshot().await,
            Err(ProjectReadError::Registry(_))
        ));
        assert!(matches!(
            fixture.reader.begin_snapshot().await,
            Err(ProjectReadError::UnfinishedOperation)
        ));
        fixture.reader.close().await?;
        assert_eq!(
            std::fs::read(fixture.location.database_path().as_path())?,
            before
        );
        fixture.writer.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn unfinished_guard_requires_close_and_then_allows_move_reselection() -> TestResult {
    let mut fixture = fixture().await?;
    drop(fixture.reader.begin_snapshot().await?);
    assert!(matches!(
        fixture.reader.begin_snapshot().await,
        Err(ProjectReadError::UnfinishedOperation)
    ));
    fixture.reader.close().await?;
    fixture.writer.close().await?;
    let moved_parent = tempfile::tempdir()?;
    let moved = moved_parent.path().join("moved");
    std::fs::rename(fixture.directory.path(), &moved)?;
    let location =
        ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(&moved)?)?;
    let migrations = migrations();
    let domains = PreparedDomains::prepare(&[DomainManifest {
        domain: "snapshot",
        migrations: &migrations,
    }])
    .await?;
    let mut reader = ProjectStoreReader::open(&location, domains)
        .await?
        .into_domain_reader(&DomainManifest {
            domain: "snapshot",
            migrations: &migrations,
        })
        .await?;
    let snapshot = reader.begin_snapshot().await?;
    assert_eq!(snapshot.identity(), &fixture.identity);
    snapshot.finish().await?;
    reader.close().await?;
    Ok(())
}
