#![expect(
    clippy::disallowed_methods,
    reason = "isolated project-store connection fixtures"
)]

use super::*;
use crate::DomainManifest;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;
use std::borrow::Cow;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn migrations() -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![
            Migration::new(
                1,
                "value".into(),
                MigrationType::Simple,
                sqlx::SqlStr::from_static("CREATE TABLE reader_values (value TEXT NOT NULL);"),
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

async fn domains() -> TestResult<PreparedDomains> {
    Ok(PreparedDomains::prepare(&[DomainManifest {
        domain: "reader",
        migrations: &migrations(),
    }])
    .await?)
}

async fn fixture(directory: &std::path::Path) -> TestResult<(ProjectStoreLocation, StoreIdentity)> {
    let location =
        ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(directory)?)?;
    let identity = crate::initialize_local_store(&location).await?;
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(location.database_path().as_path()),
    )
    .await?;
    assert_eq!(domains().await?.install(&mut writer).await?, identity);
    sqlx::query("INSERT INTO reader_values VALUES (?)")
        .bind(identity.project_uuid.to_string())
        .execute(&mut writer)
        .await?;
    writer.close().await?;
    Ok((location, identity))
}

#[tokio::test]
async fn admitted_domain_receives_only_a_read_only_project_bound_connection() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (location, identity) = fixture(directory.path()).await?;
    let before = std::fs::read(location.database_path().as_path())?;
    let reader = ProjectStoreReader::open(&location, domains().await?).await?;
    assert_eq!(reader.identity(), &identity);
    let (admitted, mut connection) = reader
        .into_domain_connection(&DomainManifest {
            domain: "reader",
            migrations: &migrations(),
        })
        .await?;
    let value: String = sqlx::query_scalar("SELECT value FROM reader_values")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(
        (admitted, value),
        (identity.clone(), identity.project_uuid.to_string())
    );
    match sqlx::query("INSERT INTO reader_values VALUES ('forbidden')")
        .execute(&mut connection)
        .await
    {
        Err(sqlx::Error::Database(error)) => assert_eq!(error.code().as_deref(), Some("8")),
        result => panic!("expected read-only refusal, got {result:?}"),
    }
    connection.close().await?;
    assert_eq!(std::fs::read(location.database_path().as_path())?, before);
    Ok(())
}

#[tokio::test]
async fn mismatched_domain_name_sql_versions_and_flags_receive_no_connection() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (location, _) = fixture(directory.path()).await?;
    let before = std::fs::read(location.database_path().as_path())?;
    for variation in 0..4 {
        let mut requested = migrations();
        let name = if variation == 0 { "unknown" } else { "reader" };
        if variation == 1 {
            requested.migrations.to_mut().pop();
        }
        if variation == 2 {
            requested.migrations.to_mut()[0].sql =
                sqlx::SqlStr::from_static("CREATE TABLE changed (value TEXT);");
        }
        if variation == 3 {
            requested.no_tx = true;
        }
        let reader = ProjectStoreReader::open(&location, domains().await?).await?;
        assert!(matches!(
            reader
                .into_domain_connection(&DomainManifest {
                    domain: name,
                    migrations: &requested
                })
                .await,
            Err(crate::StoreRegistryError::IncompatibleHistory)
        ));
        assert_eq!(std::fs::read(location.database_path().as_path())?, before);
    }
    Ok(())
}

#[tokio::test]
async fn missing_bootstrap_and_changed_schema_refuse_without_creation_or_repair() -> TestResult {
    let directory = tempfile::tempdir()?;
    let location = ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(
        directory.path(),
    )?)?;
    assert!(
        ProjectStoreReader::open(&location, domains().await?)
            .await
            .is_err()
    );
    assert!(!location.database_path().as_path().exists());
    crate::initialize_local_store(&location).await?;
    let bootstrap = std::fs::read(location.database_path().as_path())?;
    assert!(
        ProjectStoreReader::open(&location, domains().await?)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(location.database_path().as_path())?,
        bootstrap
    );
    fixture(directory.path()).await?;
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(location.database_path().as_path()),
    )
    .await?;
    sqlx::query("CREATE TABLE foreign_data(value TEXT)")
        .execute(&mut writer)
        .await?;
    writer.close().await?;
    let changed = std::fs::read(location.database_path().as_path())?;
    assert!(
        ProjectStoreReader::open(&location, domains().await?)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(location.database_path().as_path())?, changed);
    Ok(())
}

#[tokio::test]
async fn independent_readers_and_explicit_close_move_reopen_preserve_binding() -> TestResult {
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a");
    let b = directory.path().join("b");
    std::fs::create_dir(&a)?;
    std::fs::create_dir(&b)?;
    let (location_a, identity_a) = fixture(&a).await?;
    let (location_b, identity_b) = fixture(&b).await?;
    assert_ne!(identity_a, identity_b);
    let reader_a = ProjectStoreReader::open(&location_a, domains().await?).await?;
    let reader_b = ProjectStoreReader::open(&location_b, domains().await?).await?;
    assert_eq!(
        (reader_a.identity(), reader_b.identity()),
        (&identity_a, &identity_b)
    );
    reader_a.close().await?;
    reader_b.close().await?;
    let moved = directory.path().join("moved");
    assert_eq!(
        (a.parent(), moved.parent()),
        (Some(directory.path()), Some(directory.path()))
    );
    std::fs::rename(&a, &moved)?;
    let location =
        ProjectStoreLocation::select_project(&AbsolutePathBuf::from_absolute_path(&moved)?)?;
    let reader = ProjectStoreReader::open(&location, domains().await?).await?;
    assert_eq!(reader.identity(), &identity_a);
    reader.close().await?;
    Ok(())
}
