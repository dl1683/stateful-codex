use super::*;
use crate::sqlite::state_preflight::archive;
use crate::sqlite::state_preflight::archive::ArchivedFact;
use pretty_assertions::assert_eq;
use sqlx::SqlSafeStr;

#[derive(Clone, Copy)]
enum LineEndings {
    Lf,
    CrLf,
}

async fn logical(
    connection: &mut SqliteConnection,
) -> (Vec<(String, String, String, Option<String>)>, Projection) {
    let schema =
        sqlx::query_as("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    let mut projection = columns(connection).await;
    projection.insert(
        "_sqlx_migrations".to_owned(),
        vec![
            "version",
            "description",
            "installed_on",
            "success",
            "checksum",
            "execution_time",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    );
    (schema, data(connection, &projection).await)
}

async fn converted(
    source_version: i64,
    endings: LineEndings,
) -> (SqliteConfig, SqlitePool, Vec<ArchivedFact>) {
    let (config, pool) = if source_version == 56 {
        legacy_fixture().await
    } else {
        let home = crate::runtime::test_support::unique_temp_dir();
        tokio::fs::create_dir_all(&home).await.unwrap();
        let config = SqliteConfig::new_for_testing(home.as_path().abs());
        let pool = config
            .open_read_write_pool(&config.state_db_path())
            .await
            .unwrap();
        let mut migrations: Vec<_> = STATE_MIGRATOR
            .iter()
            .filter(|migration| {
                migration.version
                    <= if source_version == 38 {
                        37
                    } else {
                        source_version
                    }
            })
            .cloned()
            .collect();
        if source_version == 38 {
            let recency = state_catalog::canonical(/*version*/ 39).unwrap();
            migrations.push(Migration::new(
                /*version*/ 38,
                recency.description,
                recency.migration_type,
                recency.sql,
                /*no_tx*/ false,
            ));
        }
        Migrator::with_migrations(migrations)
            .run(&pool)
            .await
            .unwrap();
        (config, pool)
    };
    if source_version == 38 {
        sqlx::query("INSERT INTO threads (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, preview, first_user_message) VALUES ('t1', '/tmp/archive.jsonl', 1, 2, 'cli', 'test', '/tmp', 'original title', 'read-only', 'on-request', 'original preview', 'hello'); UPDATE threads SET recency_at = 123, recency_at_ms = 123000 WHERE id = 't1'")
            .execute(&pool).await.unwrap();
    } else {
        populate(&pool, "running").await;
    }
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut projection = columns(&mut transaction).await;
    if source_version == 38 {
        projection.retain(|table, _| table == "threads");
    }
    let original_data = data(&mut transaction, &projection).await;
    let history = validate(&mut transaction).await.unwrap();
    for transfer in &history.transfers {
        let identity = match transfer.destination {
            Destination::CanonicalRecency => state_catalog::canonical(/*version*/ 39),
            Destination::StatefulOwner => state_catalog::fork(/*version*/ 59),
            Destination::StatefulBinding => state_catalog::fork(/*version*/ 60),
        }
        .unwrap();
        let sql = identity.sql.as_str().replace("\r\n", "\n");
        let sql = match endings {
            LineEndings::Lf => sql,
            LineEndings::CrLf => sql.replace('\n', "\r\n"),
        };
        let identity = Migration::new(
            identity.version,
            identity.description,
            identity.migration_type,
            AssertSqlSafe(sql).into_sql_str(),
            /*no_tx*/ false,
        );
        sqlx::query("UPDATE _sqlx_migrations SET description = ?, installed_on = '1999-01-02 03:04:05', execution_time = -17, checksum = ? WHERE version = ?")
            .bind(format!("original λ {}\n", transfer.source.version)).bind(identity.checksum.as_ref())
            .bind(transfer.source.version).execute(&mut *transaction).await.unwrap();
    }
    let history = validate(&mut transaction).await.unwrap();
    let expected: Vec<_> = history
        .transfers
        .iter()
        .map(|transfer| ArchivedFact {
            source: transfer.source.clone(),
            destination: transfer.destination.clone(),
        })
        .collect();
    stage(&mut transaction, &history).await.unwrap();
    // Include genuine canonical replacements at archive source versions59/60.
    let mut canonical = Migrator::with_migrations(
        (1..=60)
            .map(|version| state_catalog::canonical(version).unwrap())
            .collect(),
    );
    canonical.set_ignore_missing(/*ignore_missing*/ true);
    run(&mut transaction, &canonical, Ledger::Canonical)
        .await
        .unwrap();
    run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
        .await
        .unwrap();
    assert_eq!(data(&mut transaction, &projection).await, original_data);
    transaction.commit().await.unwrap();
    (config, pool, expected)
}

#[tokio::test]
async fn absent_archive_is_distinct_from_an_empty_or_orphan_archive() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let mut transaction = pool.begin().await.unwrap();
    assert_eq!(archive::qualify(&mut transaction).await.unwrap(), None);
    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn source_produced_archive_sets_reopen_with_exact_original_metadata_and_replacements() {
    for source in [38, 56, 59, 60] {
        for endings in [LineEndings::Lf, LineEndings::CrLf] {
            let (config, pool, expected) = converted(source, endings).await;
            pool.close().await;
            let pool = config
                .open_read_only_pool(&config.state_db_path(), /*busy_timeout*/ None)
                .await
                .unwrap();
            let mut transaction = pool.begin().await.unwrap();
            let before = logical(&mut transaction).await;
            let namespace = crate::sqlite::state_preflight::modern::PrivateHistory {
                canonical: state_ledger::read(&mut transaction, Ledger::Canonical)
                    .await
                    .unwrap()
                    .unwrap(),
                private: state_ledger::read(&mut transaction, Ledger::Stateful)
                    .await
                    .unwrap()
                    .unwrap(),
                archive: Some(expected.clone()),
            };
            assert_eq!(
                crate::sqlite::state_preflight::modern::qualify(&mut transaction)
                    .await
                    .unwrap(),
                namespace
            );
            assert_eq!(
                archive::qualify(&mut transaction).await.unwrap(),
                Some(expected)
            );
            assert_eq!(logical(&mut transaction).await, before);
            transaction.rollback().await.unwrap();
            cleanup(config, pool).await;
        }
    }
}

#[tokio::test]
async fn altered_archive_facts_and_destination_metadata_refuse_without_mutation() {
    for mutation in [
        "UPDATE _stateful_migration_archive SET source_version = NULL",
        "UPDATE _stateful_migration_archive SET source_version = -1",
        "UPDATE _stateful_migration_archive SET source_version = 56.5",
        "UPDATE _stateful_migration_archive SET destination_ledger = '_SQLX_MIGRATIONS'",
        "UPDATE _stateful_migration_archive SET destination_version = 999",
        "UPDATE _stateful_migration_archive SET installed_on = 123",
        "UPDATE _stateful_migration_archive SET success = 0",
        "UPDATE _stateful_migration_archive SET checksum = 'text'",
        "UPDATE _stateful_migration_archive SET checksum = X'00'",
        "UPDATE _stateful_migration_archive SET execution_time = 'text'",
        "UPDATE _stateful_migration_archive SET description = 'changed'",
        "UPDATE _stateful_migrations SET checksum = X'00' WHERE version = 1",
        "DELETE FROM _stateful_migrations WHERE version = 1",
        "DROP TABLE _stateful_migrations",
        "DELETE FROM _stateful_migration_archive",
        "INSERT INTO _stateful_migration_archive SELECT 59, destination_ledger, destination_version, description, installed_on, success, checksum, execution_time FROM _stateful_migration_archive",
        "UPDATE _sqlx_migrations SET checksum = (SELECT checksum FROM _stateful_migration_archive) WHERE version = 56",
        "UPDATE _sqlx_migrations SET checksum = (SELECT checksum FROM _stateful_migration_archive) WHERE version = 59",
    ] {
        let (config, pool, _) = converted(/*source_version*/ 56, LineEndings::Lf).await;
        sqlx::raw_sql(AssertSqlSafe(mutation))
            .execute(&pool)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        let before = logical(&mut transaction).await;
        assert!(
            archive::qualify(&mut transaction).await.is_err(),
            "accepted {mutation}"
        );
        assert_eq!(logical(&mut transaction).await, before);
        transaction.rollback().await.unwrap();
        cleanup(config, pool).await;
    }
}

#[tokio::test]
async fn individually_matching_rows_cannot_form_impossible_archive_histories() {
    for source in [56, 59, 60] {
        let (config, pool, _) = converted(source, LineEndings::Lf).await;
        let mut transaction = pool.begin().await.unwrap();
        // Every added row has a genuine catalog identity and exact destination
        // metadata; only the combined provenance is impossible under staging.
        sqlx::query("INSERT INTO _stateful_migration_archive SELECT 38, '_sqlx_migrations', 39, description, installed_on, success, checksum, execution_time FROM _sqlx_migrations WHERE version = 39")
            .execute(&mut *transaction).await.unwrap();
        let before = logical(&mut transaction).await;
        assert!(archive::qualify(&mut transaction).await.is_err());
        assert_eq!(logical(&mut transaction).await, before);
        transaction.rollback().await.unwrap();
        cleanup(config, pool).await;
    }
    let (config, pool, _) = converted(/*source_version*/ 60, LineEndings::Lf).await;
    sqlx::query("DELETE FROM _stateful_migration_archive WHERE source_version = 59")
        .execute(&pool)
        .await
        .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    let before = logical(&mut transaction).await;
    assert!(
        archive::qualify(&mut transaction)
            .await
            .unwrap_err()
            .to_string()
            .contains("combined")
    );
    assert_eq!(logical(&mut transaction).await, before);
    transaction.rollback().await.unwrap();
    cleanup(config, pool).await;
}
