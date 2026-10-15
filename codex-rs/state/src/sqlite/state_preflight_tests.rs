use super::*;
use crate::migrations::STATE_MIGRATOR;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::SqlSafeStr;
use sqlx::SqlitePool;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;

fn canonical_collision(version: i64) -> Migration {
    state_catalog::canonical(version).unwrap()
}

async fn fixture(last: i64) -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let migrations = (1..=last)
        .map(|version| {
            if version <= 58 {
                STATE_MIGRATOR
                    .iter()
                    .find(|migration| migration.version == version)
                    .unwrap()
                    .clone()
            } else {
                canonical_collision(version)
            }
        })
        .collect();
    Migrator::with_migrations(migrations)
        .run(&pool)
        .await
        .unwrap();
    pool
}

async fn inspect(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut transaction = pool.begin().await?;
    let result = validate(&mut transaction).await;
    transaction.rollback().await?;
    result
}

#[tokio::test]
async fn canonical_prefix_and_opaque_suffix_leave_all_rows_unchanged() {
    for last in [0, 23, 31, 33, 34, 35, 38, 42, 55, 58, 59, 60] {
        let pool = fixture(last).await;
        inspect(&pool).await.unwrap();
        if last == 60 {
            sqlx::query("INSERT INTO _sqlx_migrations VALUES (61, 'future', 'fixed time', 1, X'000102', -42), (9999, 'later', 'another time', 1, X'FF', 99)")
                .execute(&pool).await.unwrap();
            let before = snapshot(&pool).await;
            inspect(&pool).await.unwrap();
            inspect(&pool).await.unwrap();
            assert_eq!(snapshot(&pool).await, before);
        }
        pool.close().await;
    }
}

async fn snapshot(pool: &SqlitePool) -> Vec<(i64, String, String, i64, Vec<u8>, i64)> {
    sqlx::query_as("SELECT version, description, installed_on, success, checksum, execution_time FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn recognized_legacy_identities_and_line_endings_pass_ledger_preflight() {
    for owner_version in [56, 59] {
        let pool = fixture(if owner_version == 56 { 55 } else { 58 }).await;
        let owner = STATE_MIGRATOR
            .iter()
            .find(|migration| migration.version == 59)
            .unwrap();
        let lf = owner.sql.as_str().replace("\r\n", "\n");
        let crlf = Migration::new(
            owner_version,
            "legacy owner".into(),
            MigrationType::Simple,
            sqlx::AssertSqlSafe(lf.replace('\n', "\r\n")).into_sql_str(),
            /*no_tx*/ false,
        );
        sqlx::raw_sql(owner.sql.clone())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO _sqlx_migrations VALUES (?, 'legacy', 'fixed', 1, ?, 8)")
            .bind(owner_version)
            .bind(crlf.checksum.as_ref())
            .execute(&pool)
            .await
            .unwrap();
        inspect(&pool).await.unwrap();
        pool.close().await;
    }
    let pool = fixture(/*last*/ 39).await;
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 38; UPDATE _sqlx_migrations SET version = 38 WHERE version = 39; DROP TABLE external_agent_config_imports")
        .execute(&pool).await.unwrap();
    inspect(&pool).await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn malformed_failed_unknown_and_incomplete_histories_refuse_without_writes() {
    for mutation in [
        "UPDATE _sqlx_migrations SET checksum = X'FF' WHERE version = 59",
        "UPDATE _sqlx_migrations SET success = 0 WHERE version = 59",
        "UPDATE _sqlx_migrations SET success = 2 WHERE version = 59",
        "UPDATE _sqlx_migrations SET success = 'true' WHERE version = 59",
        "UPDATE _sqlx_migrations SET version = NULL WHERE version = 59",
        "UPDATE _sqlx_migrations SET checksum = 'text' WHERE version = 59",
        "UPDATE _sqlx_migrations SET execution_time = 'text' WHERE version = 59",
        "DELETE FROM _sqlx_migrations WHERE version = 38",
        "DELETE FROM _sqlx_migrations WHERE version = 59; INSERT INTO _sqlx_migrations VALUES (61, 'future', 'fixed', 1, X'FF', 0)",
        "CREATE TRIGGER bad_ledger AFTER UPDATE ON _sqlx_migrations BEGIN DELETE FROM _sqlx_migrations; END",
    ] {
        let pool = fixture(/*last*/ 60).await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(mutation))
            .execute(&pool)
            .await
            .unwrap();
        // SQLite quote() preserves both value and storage class, including NULL.
        let before: Vec<String> = sqlx::query_scalar("SELECT quote(version)||quote(description)||quote(installed_on)||quote(success)||quote(checksum)||quote(execution_time) FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool).await.unwrap();
        assert!(inspect(&pool).await.is_err(), "accepted {mutation}");
        let after: Vec<String> = sqlx::query_scalar("SELECT quote(version)||quote(description)||quote(installed_on)||quote(success)||quote(checksum)||quote(execution_time) FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool).await.unwrap();
        assert_eq!(after, before, "mutation: {mutation}");
        pool.close().await;
    }
}

#[tokio::test]
async fn view_or_nonstandard_ledger_is_refused() {
    for schema in [
        "CREATE VIEW _sqlx_migrations AS SELECT 1 AS version",
        "CREATE TABLE _sqlx_migrations (version BIGINT, checksum BLOB)",
        "CREATE TABLE unrelated (value TEXT)",
    ] {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(schema))
            .execute(&pool)
            .await
            .unwrap();
        assert!(inspect(&pool).await.is_err(), "accepted {schema}");
        pool.close().await;
    }
}

#[tokio::test]
async fn qualification_reads_committed_wal_without_mutating_ledger() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let _cleanup = scopeguard::guard(home.clone(), |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let config = SqliteConfig::new_for_testing(home.as_path().abs());
    let path = config.state_db_path();
    let pool = config.open_read_write_pool(&path).await.unwrap();
    STATE_MIGRATOR.run(&pool).await.unwrap();
    sqlx::query("PRAGMA wal_autocheckpoint = 0")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET success = 2 WHERE version = 60")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        tokio::fs::metadata(path.with_extension("sqlite-wal"))
            .await
            .unwrap()
            .len()
            > 0
    );
    let before = snapshot(&pool).await;
    let error = check(&config, &path)
        .await
        .expect_err("committed WAL-only invalid fact must refuse");
    assert!(
        error
            .to_string()
            .contains("unsuccessful state migration 60")
    );
    assert!(!crate::is_sqlite_corruption_error(&error));
    assert_eq!(crate::runtime_db_path_for_corruption_error(&error), None);
    assert_eq!(snapshot(&pool).await, before);
    pool.close().await;
}

#[tokio::test]
async fn interrupted_owner_repairs_through_55_56_and_57_have_proven_schema() {
    for last in [55, 56, 57] {
        let home = crate::runtime::test_support::unique_temp_dir();
        tokio::fs::create_dir_all(&home).await.unwrap();
        let _cleanup = scopeguard::guard(home.clone(), |home| {
            let _ = std::fs::remove_dir_all(home);
        });
        let config = SqliteConfig::new_for_testing(home.as_path().abs());
        let path = config.state_db_path();
        tokio::fs::write(
            &path,
            include_bytes!("../../testdata/state_v56_from_4ea34e7d10.sqlite"),
        )
        .await
        .unwrap();
        let pool = config.open_read_write_pool(&path).await.unwrap();
        crate::migrations::repair_legacy_backfill_owner_migration_version(&pool, &STATE_MIGRATOR)
            .await
            .unwrap();
        let mut partial = Migrator::with_migrations(
            STATE_MIGRATOR
                .iter()
                .filter(|migration| migration.version <= last)
                .cloned()
                .collect(),
        );
        partial.set_ignore_missing(/*ignore_missing*/ true);
        config.run_migrations(&pool, &partial).await.unwrap();
        let before = snapshot(&pool).await;
        pool.close().await;
        check(&config, &path).await.unwrap();
        let read = config
            .open_read_only_pool(&path, /*busy_timeout*/ None)
            .await
            .unwrap();
        assert_eq!(snapshot(&read).await, before);
        read.close().await;
        let resumed = config
            .open_state_db(
                &crate::migrations::runtime_state_migrator(),
                /*telemetry_override*/ None,
            )
            .await
            .expect("the unchanged old startup can complete this committed intermediate");
        resumed.close().await;
    }
}

#[tokio::test]
async fn interrupted_recency_renumber_requires_absent_imports_schema() {
    let pool = fixture(/*last*/ 39).await;
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 38")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        inspect(&pool).await.is_err(),
        "arbitrary ledger deletion must refuse"
    );
    sqlx::query("DROP TABLE external_agent_config_imports")
        .execute(&pool)
        .await
        .unwrap();
    inspect(&pool).await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn fork_columns_and_trigger_require_exact_facts_and_definitions() {
    for altered_sql in [
        "owner_token INTEGER",
        "owner_token TEXT NOT NULL DEFAULT ''",
        "owner_token TEXT DEFAULT 'unexplained'",
    ] {
        let pool = fixture(/*last*/ 58).await;
        let owner = STATE_MIGRATOR
            .iter()
            .find(|migration| migration.version == 59)
            .unwrap();
        let sql = format!("ALTER TABLE backfill_state ADD COLUMN {altered_sql}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO _sqlx_migrations VALUES (59, 'owner', 'fixed', 1, ?, 0)")
            .bind(owner.checksum.as_ref())
            .execute(&pool)
            .await
            .unwrap();
        let before = snapshot(&pool).await;
        assert!(inspect(&pool).await.is_err(), "accepted {altered_sql}");
        assert_eq!(snapshot(&pool).await, before);
        pool.close().await;
    }
    let pool = fixture(/*last*/ 58).await;
    let fork = STATE_MIGRATOR
        .iter()
        .filter(|migration| migration.version >= 59);
    for migration in fork {
        sqlx::raw_sql(migration.sql.clone())
            .execute(&pool)
            .await
            .unwrap();
    }
    assert!(
        inspect(&pool).await.is_err(),
        "schema alone cannot prove fork facts"
    );
    sqlx::query("INSERT INTO _sqlx_migrations VALUES (59, 'owner', 'fixed', 1, ?, 0), (60, 'binding', 'fixed', 1, ?, 0)")
        .bind(STATE_MIGRATOR.iter().find(|migration| migration.version == 59).unwrap().checksum.as_ref())
        .bind(STATE_MIGRATOR.iter().find(|migration| migration.version == 60).unwrap().checksum.as_ref())
        .execute(&pool).await.unwrap();
    inspect(&pool).await.unwrap();
    sqlx::query("DROP TRIGGER threads_project_binding_generation; CREATE TRIGGER threads_project_binding_generation AFTER UPDATE OF project_id ON threads BEGIN UPDATE threads SET project_binding_generation = 1 WHERE id = NEW.id; END")
        .execute(&pool).await.unwrap();
    assert!(
        inspect(&pool).await.is_err(),
        "altered binding trigger must refuse"
    );
    pool.close().await;
}

#[tokio::test]
async fn canonical_collision_checksums_accept_crlf_without_rewriting_rows() {
    let pool = fixture(/*last*/ 60).await;
    for version in [59, 60] {
        let source = canonical_collision(version);
        let crlf = Migration::new(
            version,
            source.description,
            MigrationType::Simple,
            sqlx::AssertSqlSafe(
                source
                    .sql
                    .as_str()
                    .replace("\r\n", "\n")
                    .replace('\n', "\r\n"),
            )
            .into_sql_str(),
            /*no_tx*/ false,
        );
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(crlf.checksum.as_ref())
            .bind(version)
            .execute(&pool)
            .await
            .unwrap();
    }
    let before = snapshot(&pool).await;
    inspect(&pool).await.unwrap();
    assert_eq!(snapshot(&pool).await, before);
    pool.close().await;
}

#[tokio::test]
async fn retained_legacy_owner_fixture_matches_cumulative_schema() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let _cleanup = scopeguard::guard(home.clone(), |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let config = SqliteConfig::new_for_testing(home.as_path().abs());
    let path = config.state_db_path();
    let source = include_bytes!("../../testdata/state_v56_from_4ea34e7d10.sqlite");
    tokio::fs::write(&path, source).await.unwrap();
    check(&config, &path).await.unwrap();
    assert_eq!(tokio::fs::read(&path).await.unwrap(), source);
}

#[tokio::test]
async fn unicode_identifier_and_case_insensitive_fork_collisions_refuse() {
    let pool = fixture(/*last*/ 58).await;
    sqlx::query("ALTER TABLE backfill_state ADD COLUMN owner_token\u{a0} TEXT")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations VALUES (59, 'owner', 'fixed', 1, ?, 0)")
        .bind(
            state_catalog::fork(/*version*/ 59)
                .unwrap()
                .checksum
                .as_ref(),
        )
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        inspect(&pool).await.is_err(),
        "Unicode identifier is not owner_token"
    );
    pool.close().await;
    for collision in [
        "ALTER TABLE backfill_state ADD COLUMN \"OwNeR_ToKeN\" TEXT",
        "ALTER TABLE threads ADD COLUMN \"PROJECT_BINDING_GENERATION\" INTEGER",
        "CREATE TRIGGER ThReAdS_PrOjEcT_BiNdInG_GeNeRaTiOn AFTER UPDATE ON threads BEGIN SELECT 1; END",
    ] {
        let pool = fixture(/*last*/ 60).await;
        sqlx::query("INSERT INTO _sqlx_migrations VALUES (9999, 'future', 'fixed', 1, X'00', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(collision))
            .execute(&pool)
            .await
            .unwrap();
        assert!(inspect(&pool).await.is_err(), "accepted {collision}");
        pool.close().await;
    }
}

#[tokio::test]
async fn unexplained_schema_collection_is_bounded_but_opaque_objects_are_preserved() {
    let pool = fixture(/*last*/ 60).await;
    for i in 0..128 {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE TABLE extra_{i}(value TEXT)"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }
    let error = inspect(&pool).await.unwrap_err();
    assert!(error.to_string().contains("collection bounds"));
    sqlx::query("INSERT INTO _sqlx_migrations VALUES (61, 'future', 'fixed', 1, X'00', -4)")
        .execute(&pool)
        .await
        .unwrap();
    let before = snapshot(&pool).await;
    inspect(&pool).await.unwrap();
    assert_eq!(snapshot(&pool).await, before);
    pool.close().await;
}

#[tokio::test]
async fn actual_recency_repair_can_close_and_reopen_before_missing_38_runs() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let _cleanup = scopeguard::guard(home.clone(), |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let config = SqliteConfig::new_for_testing(home.as_path().abs());
    let path = config.state_db_path();
    let pool = config.open_read_write_pool(&path).await.unwrap();
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
    crate::migrations::repair_legacy_recency_migration_version(&pool, &STATE_MIGRATOR)
        .await
        .unwrap();
    let before = snapshot(&pool).await;
    pool.close().await;
    check(&config, &path).await.unwrap();
    let read = config
        .open_read_only_pool(&path, /*busy_timeout*/ None)
        .await
        .unwrap();
    assert_eq!(snapshot(&read).await, before);
    read.close().await;
    let resumed = config
        .open_state_db(
            &crate::migrations::runtime_state_migrator(),
            /*telemetry_override*/ None,
        )
        .await
        .expect("the unchanged old startup can complete this committed intermediate");
    resumed.close().await;
}

async fn apply_fork_fact(pool: &SqlitePool, version: i64) {
    let migration = state_catalog::fork(version).unwrap();
    sqlx::raw_sql(migration.sql.clone())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO _sqlx_migrations VALUES (?, ?, 'fixed', 1, ?, 0)")
        .bind(version)
        .bind(migration.description.as_ref())
        .bind(migration.checksum.as_ref())
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn binding_requires_complete_58_and_opaque_suffix_cannot_hide_legacy_transfer() {
    for last in [55, 56, 57, 58] {
        let pool = fixture(last).await;
        apply_fork_fact(&pool, /*version*/ 59).await;
        if last < 58 {
            apply_fork_fact(&pool, /*version*/ 60).await;
            assert!(
                inspect(&pool).await.is_err(),
                "binding accepted after {last}"
            );
        } else {
            inspect(&pool).await.unwrap();
            sqlx::query(
                "INSERT INTO _sqlx_migrations VALUES (9999, 'future', 'fixed', 1, X'00', 0)",
            )
            .execute(&pool)
            .await
            .unwrap();
            assert!(
                inspect(&pool).await.is_err(),
                "opaque suffix hid an old fork fact"
            );
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn sqlite3504_inputs_and_mixed_engine_extension_match_reference() {
    let cases: &[(&[u8], i64)] = &[
        (
            include_bytes!("../../testdata/state_canonical33_sqlite3504.sqlite"),
            33,
        ),
        (
            include_bytes!("../../testdata/state_canonical55_sqlite3504.sqlite"),
            55,
        ),
        (
            include_bytes!("../../testdata/state_canonical60_sqlite3504.sqlite"),
            60,
        ),
    ];
    for (bytes, last) in cases {
        let home = crate::runtime::test_support::unique_temp_dir();
        tokio::fs::create_dir_all(&home).await.unwrap();
        let _cleanup = scopeguard::guard(home.clone(), |home| {
            let _ = std::fs::remove_dir_all(home);
        });
        let config = SqliteConfig::new_for_testing(home.as_path().abs());
        let path = config.state_db_path();
        tokio::fs::write(&path, bytes).await.unwrap();
        check(&config, &path).await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), *bytes);
        let pool = config.open_read_write_pool(&path).await.unwrap();
        let reader: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await
            .unwrap();
        eprintln!("source SQLite=3.50.4, endpoint={last}, SQLx reader SQLite={reader}");
        if *last == 33 {
            let through55 = Migrator::with_migrations(
                STATE_MIGRATOR
                    .iter()
                    .filter(|migration| migration.version <= 55)
                    .cloned()
                    .collect(),
            );
            config.run_migrations(&pool, &through55).await.unwrap();
        }
        pool.close().await;
        check(&config, &path).await.unwrap();
    }
}

#[tokio::test]
async fn reserved_ledger_name_cannot_hide_a_trigger_on_another_table() {
    let pool = fixture(/*last*/ 58).await;
    sqlx::query(
        "CREATE TRIGGER _sqlx_migrations AFTER INSERT ON threads BEGIN DELETE FROM projects; END",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before = snapshot(&pool).await;
    let error = inspect(&pool).await.unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
    assert_eq!(snapshot(&pool).await, before);
    pool.close().await;
}

#[tokio::test]
async fn alternate_case_ledger_preserves_empty_and_opaque_trigger_policy() {
    for last in [0, 60] {
        let pool = fixture(last).await;
        sqlx::query("ALTER TABLE _sqlx_migrations RENAME TO ledger_case_bridge; ALTER TABLE ledger_case_bridge RENAME TO _SQLX_MIGRATIONS")
            .execute(&pool).await.unwrap();
        if last == 60 {
            sqlx::query(
                "INSERT INTO _sqlx_migrations VALUES (61, 'future', 'fixed', 1, X'00', -4)",
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        inspect(&pool).await.unwrap();
        if last == 60 {
            sqlx::query("CREATE TRIGGER arbitrary_ledger_trigger AFTER UPDATE ON _SQLX_MIGRATIONS BEGIN SELECT 1; END")
                .execute(&pool).await.unwrap();
            let before = snapshot(&pool).await;
            let error = inspect(&pool).await.unwrap_err();
            assert!(error.to_string().contains("ledger trigger"));
            assert_eq!(snapshot(&pool).await, before);
        }
        pool.close().await;
    }
}
