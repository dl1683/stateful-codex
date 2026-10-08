use std::borrow::Cow;

use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;

use super::MigrationHistorySnapshot;
use super::SqliteConfig;

#[tokio::test]
async fn migration_history_snapshot_survives_concurrent_initialization() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let _cleanup = scopeguard::guard(home.clone(), |path| {
        let _ = std::fs::remove_dir_all(path);
    });
    let path = home.join("concurrent.sqlite");
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let writer = sqlite.open_read_write_pool(&path).await.unwrap();
    // SQLx creates its empty ledger before committing the first migration.
    sqlx::raw_sql("CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY, description TEXT NOT NULL, installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, success BOOLEAN NOT NULL, checksum BLOB NOT NULL, execution_time BIGINT NOT NULL)")
        .execute(&writer).await.unwrap();
    let reader = sqlite
        .open_read_only_pool(&path, /*busy_timeout*/ None)
        .await
        .unwrap();
    let snapshot = MigrationHistorySnapshot::read(&reader).await.unwrap();
    assert_eq!(snapshot.applied, Vec::new());
    let migrator = Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            /*version*/ 1,
            "first".into(),
            MigrationType::Simple,
            "CREATE TABLE application (value TEXT);".into_sql_str(),
            /*no_tx*/ false,
        )]),
        ..Migrator::DEFAULT
    };
    sqlite.run_migrations(&writer, &migrator).await.unwrap();
    // The actual schema-classification stage runs after the independent writer committed.
    snapshot.validate(&path, &migrator).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&writer)
            .await
            .unwrap(),
        1
    );
    reader.close().await;
    sqlite
        .check_migration_history(&path, &migrator)
        .await
        .unwrap();
    // Retain the unknown nonempty-store refusal in the same concurrency fixture.
    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&writer)
        .await
        .unwrap();
    assert!(
        sqlite
            .check_migration_history(&path, &migrator)
            .await
            .unwrap_err()
            .to_string()
            .contains("missing or empty migration ledger for a nonempty database")
    );
    writer.close().await;
}
