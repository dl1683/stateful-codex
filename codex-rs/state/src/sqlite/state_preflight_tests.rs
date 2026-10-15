use super::*;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

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
    validate(&mut transaction).await
}

#[tokio::test]
async fn canonical_prefix_and_opaque_suffix_leave_all_rows_unchanged() {
    for last in [0, 38, 55, 58, 59, 60] {
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
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 38; UPDATE _sqlx_migrations SET version = 38 WHERE version = 39")
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
async fn committed_wal_refusal_precedes_writable_hooks_and_preserves_error_boundary() {
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
    let error = config
        .open_state_db(
            &crate::migrations::runtime_state_migrator(),
            /*telemetry_override*/ None,
        )
        .await
        .expect_err("committed WAL-only invalid fact must refuse");
    assert!(
        error
            .to_string()
            .contains("unsuccessful state migration 60")
    );
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(!crate::is_sqlite_corruption_error(&error));
    assert_eq!(crate::runtime_db_path_for_corruption_error(&error), None);
    assert_eq!(snapshot(&pool).await, before);
    pool.close().await;
}
