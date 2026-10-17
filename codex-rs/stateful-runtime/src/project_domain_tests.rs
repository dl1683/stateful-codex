use super::*;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreReader;
use codex_project_store::ProjectStoreWriter;
use codex_project_store::initialize_local_store;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::migrate::Migration;
use sqlx::sqlite::SqliteConnectOptions;

#[test]
fn changed_base_ownership_refuses() {
    for mutation in 0..9 {
        let mut migrations = crate::storage::MIGRATOR.migrations.to_vec();
        match mutation {
            0 => {
                migrations.remove(0);
            }
            1 => migrations.push(migrations[0].clone()),
            2 => migrations[0].sql = sqlx::SqlStr::from_static("SELECT 1;"),
            3 => migrations[0].description = Cow::Borrowed("other owner"),
            4 => migrations[0].no_tx = true,
            5 => migrations[0].migration_type = MigrationType::ReversibleUp,
            6 => migrations[0].checksum = Cow::Owned(vec![0; 48]),
            7 => {
                migrations[0].sql = sqlx::SqlStr::from_static("SELECT 1;");
                migrations[0].checksum = Cow::Owned(Sha384::digest("SELECT 1;").to_vec());
            }
            8 => migrations[0].version = 9,
            _ => unreachable!(),
        }
        let native = Migrator {
            migrations: Cow::Owned(migrations),
            ..Migrator::DEFAULT
        };
        assert!(
            matches!(
                select_base(&native),
                Err(StoreRegistryError::IncompatibleHistory)
            ),
            "mutation {mutation}"
        );
    }
}

#[tokio::test]
#[expect(
    clippy::disallowed_methods,
    reason = "disposable canonical predecessor fixture"
)]
async fn future_native_migrations_leave_populated_canonical_store_readable()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let location = ProjectStoreLocation::select_project(&directory.path().abs())?;
    let identity = initialize_local_store(&location).await?;
    // Independent pre-change owner, without invoking the candidate selector.
    let original = DomainManifest {
        domain: "stateful-runtime",
        migrations: &crate::storage::MIGRATOR,
    };
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .foreign_keys(/*on*/ true),
    )
    .await?;
    PreparedDomains::prepare(&[original])
        .await?
        .install(&mut connection)
        .await?;
    sqlx::query("INSERT INTO stateful_runs(id,project_id,goal,mode,status,strategy_revision,revision,created_at_ms,updated_at_ms) VALUES ('retained',?,'Exact α goal','collaborative','running',0,1,1,1)")
        .bind(identity.project_uuid.to_string()).execute(&mut connection).await?;
    sqlx::query("INSERT INTO stateful_run_threads VALUES ('retained',0,'thread')")
        .execute(&mut connection)
        .await?;
    let before: Vec<(String, String, String, Option<String>)> =
        sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
            .fetch_all(&mut connection)
            .await?;
    let history: Vec<(String, i64, Vec<u8>)> = sqlx::query_as(
        "SELECT domain,version,digest FROM project_store_migrations ORDER BY domain,version",
    )
    .fetch_all(&mut connection)
    .await?;

    let mut migrations = crate::storage::MIGRATOR.migrations.to_vec();
    for version in [5, 14] {
        migrations.push(Migration::new(
            version,
            "fixture native only".into(),
            MigrationType::Simple,
            sqlx::SqlStr::from_static("CREATE TABLE native_only(value TEXT);"),
            /*no_tx*/ false,
        ));
    }
    migrations.sort_by_key(|migration| migration.version);
    let native = Migrator {
        migrations: Cow::Owned(migrations),
        ignore_missing: true,
        locking: false,
        no_tx: true,
        ..Migrator::DEFAULT
    };
    let frozen = select_base(&native)?;
    let canonical = DomainManifest {
        domain: "stateful-runtime",
        migrations: &frozen,
    };
    let mut reader =
        ProjectStoreReader::open(&location, PreparedDomains::prepare(&[canonical]).await?)
            .await?
            .into_domain_reader(&project_store_domain()?)
            .await?;
    let mut snapshot = reader.begin_snapshot().await?;
    snapshot.require_domain(&project_store_domain()?)?;
    assert_eq!(snapshot.identity(), &identity);
    assert_eq!(
        sqlx::query_as::<_, (String, String)>(
            "SELECT project_id,goal FROM stateful_runs WHERE id='retained'"
        )
        .fetch_one(snapshot.connection())
        .await?,
        (
            identity.project_uuid.to_string(),
            "Exact α goal".to_string()
        )
    );
    snapshot.finish().await?;
    reader.close().await?;
    assert_eq!(
        sqlx::query_as::<_, (String, String, String, Option<String>)>(
            "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name"
        )
        .fetch_all(&mut connection)
        .await?,
        before
    );
    assert_eq!(
        sqlx::query_as::<_, (String, i64, Vec<u8>)>(
            "SELECT domain,version,digest FROM project_store_migrations ORDER BY domain,version"
        )
        .fetch_all(&mut connection)
        .await?,
        history
    );
    connection.close().await?;

    // Exercise actual canonical guards against the simulated native selection,
    // retaining an independently installed predecessor. No native opener runs.
    let domains = || DomainManifest {
        domain: "stateful-runtime",
        migrations: &frozen,
    };
    let store = crate::StatefulRunStore {
        pool: sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(/*max*/ 1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(location.database_path().as_path())
                    .foreign_keys(/*on*/ true),
            )
            .await?,
    };
    let run_id = crate::StatefulRunId::parse("retained")?;
    let commit = crate::acceptance_test_support::settled_commit(
        &store, &run_id, "owner", /*validated_obligation_sequence*/ None,
    )
    .await;
    let expected_ledger = store.acceptance_ledger(&run_id).await?;
    let mut reader =
        ProjectStoreReader::open(&location, PreparedDomains::prepare(&[domains()]).await?)
            .await?
            .into_domain_reader(&project_store_domain()?)
            .await?;
    assert_eq!(
        crate::read_project_acceptance_ledger(reader.begin_snapshot().await?, &run_id).await?,
        expected_ledger
    );
    reader.close().await?;
    let mut writer = ProjectStoreWriter::open(
        &location,
        PreparedDomains::prepare(&[domains()]).await?,
        &identity,
    )
    .await?;
    crate::commit_project_verification_end(
        writer.begin_write().await?,
        &run_id,
        "owner",
        commit.verification.attempt,
    )
    .await?;
    let attempt = crate::commit_project_verification_start(
        writer.begin_write().await?,
        &run_id,
        "owner",
        /*lease_ms*/ 60_000,
    )
    .await?;
    let commit = crate::AcceptanceCommit {
        verification: crate::VerificationClaim {
            owner: "owner".into(),
            attempt,
        },
        ..commit
    };
    let transaction = crate::revalidate_project_run_start(
        writer.begin_write().await?,
        &run_id,
        /*expected_created_at_ms*/ 1,
    )
    .await?;
    let (completed, obligation) = crate::commit_project_run(
        transaction,
        crate::ProjectRunCompletion {
            run_id: &run_id,
            update: crate::StatefulRunUpdate {
                expected_revision: 1,
                status: crate::StatefulRunStatus::Completed,
                strategy: None,
                result: Some("done".into()),
            },
            acceptance: &commit,
            final_obligation: None,
        },
    )
    .await?;
    let expected = crate::StatefulRun {
        status: crate::StatefulRunStatus::Completed,
        revision: 2,
        result: Some("done".into()),
        ..store.get_run(&run_id).await?.unwrap()
    };
    assert_eq!((completed, obligation), (expected, None));
    writer.close().await?;
    store.pool.close().await;
    Ok(())
}
