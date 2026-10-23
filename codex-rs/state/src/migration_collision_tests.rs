use std::borrow::Cow;

use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migrator;

use super::STATE_MIGRATOR;
use crate::SqliteConfig;
use crate::StateRuntime;

#[tokio::test]
async fn fresh_state_migrations_include_upstream_and_fork_schema_after_reopen() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let _cleanup = scopeguard::guard(home.clone(), |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let runtime = StateRuntime::init(sqlite.clone(), "test".into())
        .await
        .unwrap();
    runtime.close().await;
    let reopened = StateRuntime::init(sqlite.clone(), "test".into())
        .await
        .unwrap();
    let pool = sqlite
        .open_read_write_pool(&sqlite.state_db_path())
        .await
        .unwrap();
    let columns = sqlx::query_scalar::<_, String>(
        "SELECT name FROM pragma_table_info('backfill_state') WHERE name = 'owner_token' UNION ALL SELECT name FROM pragma_table_info('threads') WHERE name = 'project_binding_generation' ORDER BY name",
    ).fetch_all(&pool).await.unwrap();
    let upstream_index: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'index' AND name = 'idx_thread_attachments_identity_thread')",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(
        (columns, upstream_index),
        (
            vec![
                "owner_token".to_string(),
                "project_binding_generation".to_string()
            ],
            true
        )
    );
    pool.close().await;
    reopened.close().await;
}

#[tokio::test]
async fn historical_fork_59_60_store_refuses_without_rewriting_its_database() {
    let home = crate::runtime::test_support::unique_temp_dir();
    tokio::fs::create_dir_all(&home).await.unwrap();
    let _cleanup = scopeguard::guard(home.clone(), |home| {
        let _ = std::fs::remove_dir_all(home);
    });
    let sqlite = SqliteConfig::new_for_testing(home.as_path().abs());
    let path = sqlite.state_db_path();
    let pool = sqlite.open_read_write_pool(&path).await.unwrap();
    let historical = Migrator {
        migrations: Cow::Owned(
            STATE_MIGRATOR
                .iter()
                .filter(|migration| migration.version <= 58 || matches!(migration.version, 62 | 63))
                .cloned()
                .map(|mut migration| {
                    migration.version = match migration.version {
                        62 => 59,
                        63 => 60,
                        version => version,
                    };
                    migration
                })
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    historical.run(&pool).await.unwrap();
    sqlx::query("UPDATE backfill_state SET status = 'complete', last_watermark = 'legacy-evidence', owner_token = 'legacy-owner' WHERE id = 1").execute(&pool).await.unwrap();
    pool.close().await;
    let before = std::fs::read(&path).unwrap();
    let error = match StateRuntime::init(sqlite, "test".into()).await {
        Ok(runtime) => {
            runtime.close().await;
            panic!("historical fork slots must not be silently converted");
        }
        Err(error) => error,
    };
    assert!(error.to_string().contains("migration 59"), "{error:#}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
