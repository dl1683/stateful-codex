#![expect(
    clippy::disallowed_methods,
    reason = "isolated project-store transaction fixtures"
)]

#[path = "writer/upgrade_tests.rs"]
mod upgrade_tests;

use super::*;
use crate::DomainManifest;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;
use std::borrow::Cow;
use std::time::Instant;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn domains() -> TestResult<PreparedDomains> {
    let manifest = |sql| Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            /*version*/ 1,
            "domain".into(),
            MigrationType::Simple,
            sqlx::SqlStr::from_static(sql),
            /*no_tx*/ false,
        )]),
        ..Migrator::DEFAULT
    };
    Ok(PreparedDomains::prepare(&[
        DomainManifest {
            domain: "alpha",
            migrations: &manifest("CREATE TABLE alpha(id INTEGER PRIMARY KEY, value TEXT NOT NULL);"),
        },
        DomainManifest {
            domain: "bravo",
            migrations: &manifest("CREATE TABLE bravo(id INTEGER PRIMARY KEY REFERENCES alpha(id), value TEXT NOT NULL);"),
        },
    ])
    .await?)
}

async fn fixture() -> TestResult<(tempfile::TempDir, ProjectStoreLocation, StoreIdentity)> {
    let directory = tempfile::tempdir()?;
    let location = ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(
        directory.path(),
    )?)?;
    let identity = crate::initialize_local_store(&location).await?;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(location.database_path().as_path()),
    )
    .await?;
    assert_eq!(domains().await?.install(&mut connection).await?, identity);
    connection.close().await?;
    Ok((directory, location, identity))
}

async fn read_values(location: &ProjectStoreLocation) -> TestResult<Vec<(i64, String, String)>> {
    let reader = ProjectStoreReader::open(location, domains().await?).await?;
    let mut connection = reader.connection;
    let rows = sqlx::query_as("SELECT alpha.id, alpha.value, bravo.value FROM alpha JOIN bravo USING(id) ORDER BY alpha.id")
        .fetch_all(&mut connection).await?;
    connection.close().await?;
    Ok(rows)
}

#[tokio::test]
async fn commit_is_atomic_across_domains_and_read_snapshots() -> TestResult {
    let (_directory, location, identity) = fixture().await?;
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    let reader = ProjectStoreReader::open(&location, domains().await?).await?;
    let mut connection = reader.connection;
    let mut snapshot = connection.begin().await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM alpha")
        .fetch_one(&mut *snapshot)
        .await?;
    assert_eq!(count, 0);
    let mut transaction = writer.begin_write().await?;
    assert_eq!(transaction.identity(), &identity);
    sqlx::raw_sql(
        "INSERT INTO alpha VALUES(1, 'knowledge'); INSERT INTO bravo VALUES(1, 'terminal');",
    )
    .execute(transaction.connection())
    .await?;
    transaction.commit().await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM bravo")
        .fetch_one(&mut *snapshot)
        .await?;
    assert_eq!(count, 0);
    snapshot.commit().await?;
    connection.close().await?;
    assert_eq!(
        read_values(&location).await?,
        vec![(1, "knowledge".into(), "terminal".into())]
    );
    writer.begin_write().await?.rollback().await?;
    writer.close().await?;
    Ok(())
}

#[tokio::test]
async fn explicit_rollback_and_dropped_guard_publish_nothing() -> TestResult {
    let (_directory, location, identity) = fixture().await?;
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    for explicit in [true, false] {
        let mut transaction = writer.begin_write().await?;
        sqlx::raw_sql(
            "INSERT INTO alpha VALUES(1, 'proposed'); INSERT INTO bravo VALUES(1, 'proposed');",
        )
        .execute(transaction.connection())
        .await?;
        assert!(
            sqlx::query("INSERT INTO bravo VALUES(2, 'invalid')")
                .execute(transaction.connection())
                .await
                .is_err()
        );
        if explicit {
            transaction.rollback().await?;
        } else {
            drop(transaction);
            assert!(matches!(
                writer.begin_write().await,
                Err(ProjectWriteError::UnfinishedOperation)
            ));
        }
    }
    writer.close().await?;
    assert_eq!(read_values(&location).await?, vec![]);
    let mut reopened = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    reopened.begin_write().await?.rollback().await?;
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn wrong_identity_or_unknown_store_refuses_before_journal_changes() -> TestResult {
    let (_directory, location, mut identity) = fixture().await?;
    let before = std::fs::read(location.database_path().as_path())?;
    identity.store_uuid = uuid::Uuid::new_v4();
    assert!(matches!(
        ProjectStoreWriter::open(&location, domains().await?, &identity).await,
        Err(ProjectWriteError::IdentityMismatch)
    ));
    assert_eq!(std::fs::read(location.database_path().as_path())?, before);
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(location.database_path().as_path()),
    )
    .await?;
    sqlx::query("CREATE TABLE unknown(value TEXT)")
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let before = std::fs::read(location.database_path().as_path())?;
    assert!(
        ProjectStoreWriter::open(&location, domains().await?, &identity)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(location.database_path().as_path())?, before);
    assert!(
        !location
            .project_root()
            .join(".stateful/memory.sqlite-wal")
            .as_path()
            .exists()
    );
    Ok(())
}

#[tokio::test]
async fn drift_refuses_before_access_and_inside_commit_without_publishing() -> TestResult {
    for mutation in [
        "UPDATE project_store_identity SET project_uuid = '00000000-0000-4000-8000-000000000001'",
        "UPDATE project_store_migrations SET digest = zeroblob(32) WHERE domain = 'alpha'",
        "CREATE TABLE unexpected(value TEXT)",
    ] {
        let (_directory, location, identity) = fixture().await?;
        let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
        let mut transaction = writer.begin_write().await?;
        sqlx::query("INSERT INTO alpha VALUES(1, 'unpublished')")
            .execute(transaction.connection())
            .await?;
        sqlx::query(mutation)
            .execute(transaction.connection())
            .await?;
        assert!(transaction.commit().await.is_err());
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
        let reader = ProjectStoreReader::open(&location, domains().await?).await?;
        let mut connection = reader.connection;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM alpha")
            .fetch_one(&mut connection)
            .await?;
        assert_eq!(count, 0);
        connection.close().await?;
        let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(location.database_path().as_path()),
        )
        .await?;
        sqlx::query(mutation).execute(&mut connection).await?;
        connection.close().await?;
        assert!(writer.begin_write().await.is_err());
        assert!(matches!(
            writer.begin_write().await,
            Err(ProjectWriteError::UnfinishedOperation)
        ));
        writer.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn busy_worker() -> TestResult {
    let Some(path) = std::env::var_os("CODEX_PROJECT_WRITER_TEST_PROJECT") else {
        return Ok(());
    };
    let location =
        ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(path)?)?;
    let reader = ProjectStoreReader::open(&location, domains().await?).await?;
    let identity = reader.identity().clone();
    reader.close().await?;
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    assert!(matches!(
        writer.begin_write().await,
        Err(ProjectWriteError::Busy)
    ));
    assert!(matches!(
        writer.begin_write().await,
        Err(ProjectWriteError::UnfinishedOperation)
    ));
    writer.close().await?;
    Ok(())
}

#[tokio::test]
async fn independent_process_busy_is_bounded_and_another_project_is_unlocked() -> TestResult {
    let (_directory, location, identity) = fixture().await?;
    let (_other_directory, other_location, other_identity) = fixture().await?;
    let mut writer = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    let transaction = writer.begin_write().await?;
    let mut other =
        ProjectStoreWriter::open(&other_location, domains().await?, &other_identity).await?;
    other.begin_write().await?.commit().await?;
    other.close().await?;
    let start = Instant::now();
    let output = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "writer::tests::busy_worker", "--nocapture"])
        .env(
            "CODEX_PROJECT_WRITER_TEST_PROJECT",
            location.project_root().as_path(),
        )
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(start.elapsed() < Duration::from_secs(/*secs*/ 5));
    transaction.rollback().await?;
    writer.close().await?;
    let mut reopened = ProjectStoreWriter::open(&location, domains().await?, &identity).await?;
    reopened.begin_write().await?.commit().await?;
    reopened.close().await?;
    Ok(())
}
