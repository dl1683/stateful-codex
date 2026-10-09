use std::borrow::Cow;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::AssertSqlSafe;
use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::DATABASE_NAME;
use super::MIGRATOR;
use super::StatefulRunStore;

#[tokio::test]
async fn migration_history_runtime_common_prefix_reopens_with_integrity() {
    for version in [0, 3, 4, 10, 11] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        if version > 0 {
            let prefix = Migrator {
                migrations: Cow::Owned(
                    MIGRATOR
                        .iter()
                        .filter(|m| m.version <= version)
                        .cloned()
                        .collect(),
                ),
                ..Migrator::DEFAULT
            };
            let pool = sqlite
                .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
                .await
                .unwrap();
            sqlite.run_migrations(&pool, &prefix).await.unwrap();
            sqlx::query("INSERT INTO stateful_runs (id, project_id, goal, mode, status, strategy_revision, revision, created_at_ms, updated_at_ms) VALUES ('legacy', 'project', 'Exact goal: α.', 'collaborative', 'running', 0, 1, 1, 1)")
                .execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO stateful_run_threads VALUES ('legacy', 0, 'thread')")
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
        }
        let store = StatefulRunStore::open(&sqlite).await.unwrap();
        let id = crate::StatefulRunId::parse("legacy").unwrap();
        let before = store.get_run(&id).await.unwrap();
        if version > 0 {
            assert_eq!(before.as_ref().unwrap().value.goal, "Exact goal: α.");
        }
        store.pool.close().await;
        let reopened = StatefulRunStore::open(&sqlite).await.unwrap();
        assert_eq!(reopened.get_run(&id).await.unwrap(), before);
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
                .fetch_one(&reopened.pool)
                .await
                .unwrap(),
            "ok"
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&reopened.pool)
                .await
                .unwrap()
                .is_empty()
        );
        reopened.pool.close().await;
    }
}

#[tokio::test]
async fn migration_history_runtime_future_and_edited_histories_refuse_unchanged() {
    let variants = [
        (
            5,
            "context windows",
            include_str!("../tests/data/migration_history/early_0005.sql"),
        ),
        (
            5,
            "context windows",
            include_str!("../tests/data/migration_history/final_0005.sql"),
        ),
        (
            6,
            "window journal",
            include_str!("../tests/data/migration_history/early_0006.sql"),
        ),
        (
            6,
            "window journal",
            include_str!("../tests/data/migration_history/mid_0006.sql"),
        ),
        (
            6,
            "window journal",
            include_str!("../tests/data/migration_history/final_0006.sql"),
        ),
        (
            5,
            "unknown history",
            "CREATE TABLE unknown_history (words TEXT);",
        ),
    ];
    for (version, description, sql) in variants {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let mut migrations = MIGRATOR.migrations.to_vec();
        // 0006 needs the final 0005 schema; classification must also reject it.
        if version == 6 {
            migrations.push(Migration::new(
                /*version*/ 5,
                Cow::Borrowed("context windows"),
                MigrationType::Simple,
                AssertSqlSafe(variants[1].2).into_sql_str(),
                /*no_tx*/ false,
            ));
        }
        migrations.push(Migration::new(
            version,
            Cow::Borrowed(description),
            MigrationType::Simple,
            AssertSqlSafe(sql).into_sql_str(),
            /*no_tx*/ false,
        ));
        let historic = Migrator {
            migrations: Cow::Owned(migrations),
            ..Migrator::DEFAULT
        };
        let path = sqlite.home().join(DATABASE_NAME);
        let pool = sqlite.open_read_write_pool(&path).await.unwrap();
        sqlite.run_migrations(&pool, &historic).await.unwrap();
        pool.close().await;
        let before = std::fs::read(&path).unwrap();
        let rejected = historic.iter().find(|m| m.version == 5).unwrap();
        let prefix: String = rejected
            .checksum
            .iter()
            .take(/*n*/ 12)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let error = StatefulRunStore::open(&sqlite)
            .await
            .err()
            .unwrap()
            .to_string();
        let expected = format!(
            "unsupported migration history: database={}, version=0005, description={:?}, checksum={prefix}, success=true; original store untouched; automatic conversion is unavailable",
            path.display(),
            rejected.description
        );
        assert!(error.contains(&expected), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

#[tokio::test]
async fn migration_history_runtime_nonempty_untracked_database_refuses_unchanged() {
    for empty_ledger in [false, true] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let path = sqlite.home().join(DATABASE_NAME);
        let pool = sqlite.open_read_write_pool(&path).await.unwrap();
        if empty_ledger {
            let empty = Migrator {
                migrations: Cow::Owned(Vec::new()),
                ..Migrator::DEFAULT
            };
            sqlite.run_migrations(&pool, &empty).await.unwrap();
        }
        sqlx::query("CREATE TABLE legacy_words (words TEXT); INSERT INTO legacy_words VALUES ('Keep exact words: α.')").execute(&pool).await.unwrap();
        pool.close().await;
        let before = std::fs::read(&path).unwrap();
        let error = StatefulRunStore::open(&sqlite)
            .await
            .err()
            .unwrap()
            .to_string();
        for expected in [
            "unsupported migration history",
            "version=unknown",
            "missing or empty migration ledger",
            "checksum=unavailable",
        ] {
            assert!(error.contains(expected), "{error}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

/// A run that began before effect observation existed has unknown earlier effects, so it is
/// never exempt from acceptance; a run that begins afterwards can be.
#[tokio::test]
async fn a_run_that_began_before_effect_observation_is_not_exempt() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let prefix = Migrator {
        migrations: Cow::Owned(
            MIGRATOR
                .iter()
                .filter(|m| m.version <= 10)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
        .await
        .unwrap();
    sqlite.run_migrations(&pool, &prefix).await.unwrap();
    sqlx::query("INSERT INTO stateful_runs (id, project_id, goal, mode, status, strategy_revision, revision, created_at_ms, updated_at_ms) VALUES ('legacy', 'project', 'Publish the report.', 'autonomous', 'running', 0, 1, 1, 1)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let store = StatefulRunStore::open(&sqlite).await.unwrap();
    let legacy = crate::StatefulRunId::parse("legacy").unwrap();
    let ledger = store.acceptance_ledger(&legacy).await.unwrap();
    assert!(!ledger.observations_complete);
    assert!(!crate::read_only_exempt(&ledger));
    let fresh = crate::StatefulRunId::parse("fresh").unwrap();
    store
        .create_run(
            fresh.clone(),
            crate::NewStatefulRun {
                project_id: "project".to_string(),
                thread_ids: vec!["thread".to_string()],
                goal: "What does the parser do?".to_string(),
                mode: crate::WorkflowMode::Autonomous,
                budget: crate::RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 60,
                },
            },
        )
        .await
        .unwrap();
    assert!(crate::read_only_exempt(
        &store.acceptance_ledger(&fresh).await.unwrap()
    ));
}
