use super::*;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::SqlitePool;

use super::super::validate;
use crate::SqliteConfig;
use crate::migrations::STATE_MIGRATOR;

async fn legacy_fixture() -> (SqliteConfig, SqlitePool) {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let config = SqliteConfig::new_for_testing(home.as_path().abs());
    tokio::fs::write(
        config.state_db_path(),
        include_bytes!("../../testdata/state_v56_from_4ea34e7d10.sqlite"),
    )
    .await
    .unwrap();
    let pool = config
        .open_read_write_pool(&config.state_db_path())
        .await
        .unwrap();
    (config, pool)
}

async fn cleanup(config: SqliteConfig, pool: SqlitePool) {
    pool.close().await;
    let _ = tokio::fs::remove_dir_all(config.home()).await;
}

fn canonical_range() -> Migrator {
    let mut base = Migrator::with_migrations(
        STATE_MIGRATOR
            .iter()
            .filter(|migration| migration.version <= 58)
            .cloned()
            .collect(),
    );
    base.set_ignore_missing(/*ignore_missing*/ true);
    base
}

async fn populate(pool: &SqlitePool, status: &str) {
    sqlx::query("INSERT INTO projects (id, name, metadata, position, created_at_ms, updated_at_ms) VALUES ('p1', 'first', '{}', 0, 1, 1), ('p2', 'second', '{}', 1, 1, 1); INSERT INTO threads (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, preview, first_user_message, project_id) VALUES ('t1', '/tmp/transfer.jsonl', 1, 2, 'cli', 'test', '/tmp', 'original title', 'read-only', 'on-request', 'original preview', 'hello', 'p1')")
        .execute(pool).await.unwrap();
    let (watermark, success, owner) = match status {
        "pending" => (None, None, None),
        "complete" => (Some("completed-watermark"), Some(812_i64), None),
        "running" => (
            Some("running-watermark"),
            Some(811_i64),
            Some("synthetic-owner"),
        ),
        _ => panic!("unknown test status"),
    };
    sqlx::query("UPDATE backfill_state SET status = ?, last_watermark = ?, last_success_at = ?, owner_token = ?, updated_at = ? WHERE id = 1")
        .bind(status).bind(watermark).bind(success).bind(owner).bind(chrono::Utc::now().timestamp())
        .execute(pool).await.unwrap();
}

type Projection = std::collections::BTreeMap<String, Vec<String>>;

async fn columns(connection: &mut SqliteConnection) -> Projection {
    let mut columns = Projection::new();
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB 'sqlite_*' AND name != '_sqlx_migrations' ORDER BY name")
        .fetch_all(&mut *connection).await.unwrap();
    for table in tables {
        let names = sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
            .bind(&table)
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        columns.insert(table, names);
    }
    columns
}

async fn data(connection: &mut SqliteConnection, columns: &Projection) -> Projection {
    let mut result = Projection::new();
    for (table, columns) in columns {
        let expressions = columns
            .iter()
            .map(|column| {
                let column = column.replace('"', "\"\"");
                format!("typeof(\"{column}\") || ':' || quote(\"{column}\")")
            })
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let table_name = table.replace('"', "\"\"");
        // Compare all original data with storage classes and stable value ordering.
        let rows = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT {expressions} AS snapshot FROM \"{table_name}\" ORDER BY snapshot"
        )))
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        result.insert(table.clone(), rows);
    }
    result
}

#[tokio::test]
async fn transferred_owner_facts_preserve_populated_pending_complete_and_running() {
    for status in ["pending", "complete", "running"] {
        let (config, pool) = legacy_fixture().await;
        populate(&pool, status).await;
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let projection = columns(&mut transaction).await;
        let before = data(&mut transaction, &projection).await;
        let history = validate(&mut transaction).await.unwrap();
        let original = history.transfers[0].source.clone();
        stage(&mut transaction, &history).await.unwrap();
        run(&mut transaction, &canonical_range(), Ledger::Canonical)
            .await
            .unwrap();
        run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
            .await
            .unwrap();
        assert_eq!(data(&mut transaction, &projection).await, before);
        assert!(
            crate::sqlite::state_metadata::validate(
                &mut transaction,
                crate::sqlite::state_metadata::Metadata::Archive,
            )
            .await
            .unwrap()
        );
        let mut expected = original.clone();
        expected.version = 1;
        let fork = state_ledger::read(&mut transaction, Ledger::Stateful)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fork[0], expected);
        let archive: (i64, String, i64, String, String, i64, Vec<u8>, i64) = sqlx::query_as("SELECT source_version, destination_ledger, destination_version, description, installed_on, success, checksum, execution_time FROM _stateful_migration_archive")
            .fetch_one(&mut *transaction).await.unwrap();
        assert_eq!(
            archive,
            (
                original.version,
                "_stateful_migrations".to_owned(),
                1,
                original.description,
                original.installed_on,
                original.success,
                original.checksum,
                original.execution_time
            )
        );
        transaction.commit().await.unwrap();
        let mut connection = pool.acquire().await.unwrap();
        assert_eq!(data(&mut connection, &projection).await, before);
        drop(connection);
        cleanup(config, pool).await;
    }
}

#[tokio::test]
async fn binding_generation_and_trigger_survive_transfer_without_replaying_ddl() {
    let (config, pool) = legacy_fixture().await;
    pool.close().await;
    let pool = config
        .open_state_db(
            &crate::migrations::runtime_state_migrator(),
            /*telemetry_override*/ None,
        )
        .await
        .unwrap();
    populate(&pool, "running").await;
    sqlx::query("UPDATE threads SET project_binding_generation = 37 WHERE id = 't1'")
        .execute(&pool)
        .await
        .unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let projection = columns(&mut transaction).await;
    let before = data(&mut transaction, &projection).await;
    let history = validate(&mut transaction).await.unwrap();
    stage(&mut transaction, &history).await.unwrap();
    run(&mut transaction, &canonical_range(), Ledger::Canonical)
        .await
        .unwrap();
    run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
        .await
        .unwrap();
    assert_eq!(data(&mut transaction, &projection).await, before);
    let actual = state_ledger::read(&mut transaction, Ledger::Stateful)
        .await
        .unwrap()
        .unwrap();
    let expected: Vec<_> = history
        .transfers
        .iter()
        .map(|transfer| {
            let mut record = transfer.source.clone();
            record.version = if transfer.destination == Destination::StatefulOwner {
                1
            } else {
                2
            };
            record
        })
        .collect();
    assert_eq!(actual, expected);
    transaction.commit().await.unwrap();
    sqlx::query("UPDATE threads SET project_id = 'p2' WHERE id = 't1'")
        .execute(&pool)
        .await
        .unwrap();
    let binding: (String, i64) = sqlx::query_as(
        "SELECT project_id, project_binding_generation FROM threads WHERE id = 't1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(binding, ("p2".to_owned(), 38));
    cleanup(config, pool).await;
}

#[tokio::test]
async fn outer_rollback_restores_facts_schema_and_data_after_nested_runners() {
    #[derive(Eq, PartialEq)]
    enum Point {
        Transfer,
        Canonical,
        Both,
    }
    for point in [Point::Transfer, Point::Canonical, Point::Both] {
        let (config, pool) = legacy_fixture().await;
        populate(&pool, "complete").await;
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let projection = columns(&mut transaction).await;
        let before = data(&mut transaction, &projection).await;
        let history = validate(&mut transaction).await.unwrap();
        stage(&mut transaction, &history).await.unwrap();
        if point != Point::Transfer {
            run(&mut transaction, &canonical_range(), Ledger::Canonical)
                .await
                .unwrap();
        }
        if point == Point::Both {
            run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
                .await
                .unwrap();
        }
        transaction.rollback().await.unwrap();
        let mut transaction = pool.begin().await.unwrap();
        assert_eq!(validate(&mut transaction).await.unwrap(), history);
        assert_eq!(data(&mut transaction, &projection).await, before);
        transaction.rollback().await.unwrap();
        cleanup(config, pool).await;
    }
}

#[tokio::test]
async fn stale_plan_refuses_before_creating_private_schema() {
    let (config, pool) = legacy_fixture().await;
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let history = validate(&mut transaction).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET description = 'changed' WHERE version = 56")
        .execute(&mut *transaction)
        .await
        .unwrap();
    assert!(stage(&mut transaction, &history).await.is_err());
    let private: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE '_stateful_%'")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    assert_eq!(private, 0);
    transaction.rollback().await.unwrap();
    cleanup(config, pool).await;
}

#[tokio::test]
async fn genuine_first_owner_application_resets_once_and_repeat_runner_preserves() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    run(&mut transaction, &canonical_range(), Ledger::Canonical)
        .await
        .unwrap();
    sqlx::query("UPDATE backfill_state SET status = 'complete', last_watermark = 'old', last_success_at = 812 WHERE id = 1")
        .execute(&mut *transaction).await.unwrap();
    run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
        .await
        .unwrap();
    let state: (String, Option<String>, Option<i64>, Option<String>) = sqlx::query_as("SELECT status, last_watermark, last_success_at, owner_token FROM backfill_state WHERE id = 1")
        .fetch_one(&mut *transaction).await.unwrap();
    assert_eq!(state, ("pending".to_owned(), None, None, None));
    sqlx::query("UPDATE backfill_state SET status = 'running', last_watermark = 'new', last_success_at = 813, owner_token = 'new-owner' WHERE id = 1")
        .execute(&mut *transaction).await.unwrap();
    let projection = columns(&mut transaction).await;
    let before = data(&mut transaction, &projection).await;
    run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
        .await
        .unwrap();
    assert_eq!(data(&mut transaction, &projection).await, before);
    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn runner_refuses_nontransactional_flags_and_sql_with_a_copied_checksum() {
    use sqlx::SqlSafeStr;
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut base = canonical_range();
    base.no_tx = true;
    assert!(
        run(&mut transaction, &base, Ledger::Canonical)
            .await
            .unwrap_err()
            .to_string()
            .contains("nontransactional")
    );
    base.no_tx = false;
    base.migrations.to_mut()[0].sql = "DROP TABLE projects;".into_sql_str();
    assert!(
        run(&mut transaction, &base, Ledger::Canonical)
            .await
            .unwrap_err()
            .to_string()
            .contains("unpinned")
    );
    let objects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*'")
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    assert_eq!(objects, 0);
    transaction.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn recency_alias_transfers_without_replaying_recency_data() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let mut legacy: Vec<_> = STATE_MIGRATOR
        .iter()
        .filter(|migration| migration.version <= 37)
        .cloned()
        .collect();
    let recency = state_catalog::canonical(/*version*/ 39).unwrap();
    legacy.push(Migration::new(
        /*version*/ 38,
        recency.description,
        recency.migration_type,
        recency.sql,
        /*no_tx*/ false,
    ));
    Migrator::with_migrations(legacy).run(&pool).await.unwrap();
    sqlx::query("INSERT INTO threads (id, rollout_path, created_at, updated_at, source, model_provider, cwd, title, sandbox_policy, approval_mode, preview, first_user_message) VALUES ('t1', '/tmp/recency.jsonl', 1, 2, 'cli', 'test', '/tmp', 'original title', 'read-only', 'on-request', 'original preview', 'hello'); UPDATE threads SET recency_at = 123, recency_at_ms = 123000 WHERE id = 't1'")
        .execute(&pool).await.unwrap();
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let mut projection = columns(&mut transaction).await;
    // Later canonical migrations intentionally remove obsolete pre-44 tables.
    projection.retain(|table, _| table == "threads");
    let before = data(&mut transaction, &projection).await;
    let history = validate(&mut transaction).await.unwrap();
    let original = history
        .transfers
        .iter()
        .find(|transfer| transfer.destination == Destination::CanonicalRecency)
        .unwrap()
        .source
        .clone();
    stage(&mut transaction, &history).await.unwrap();
    run(&mut transaction, &canonical_range(), Ledger::Canonical)
        .await
        .unwrap();
    run(&mut transaction, &FORK_MIGRATOR, Ledger::Stateful)
        .await
        .unwrap();
    assert_eq!(data(&mut transaction, &projection).await, before);
    let canonical = state_ledger::read(&mut transaction, Ledger::Canonical)
        .await
        .unwrap()
        .unwrap();
    let mut expected = original;
    expected.version = 39;
    assert_eq!(
        canonical
            .iter()
            .find(|record| record.version == 39)
            .unwrap(),
        &expected
    );
    transaction.commit().await.unwrap();
    pool.close().await;
}
