use std::borrow::Cow;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;
use sqlx::AssertSqlSafe;
use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::BlackboardStore;
use super::DATABASE_NAME;
use super::MIGRATOR;
use crate::BlackboardEntryId;
use crate::ContextMapStore;
use crate::HierarchyStore;
use crate::MemberOutcome;
use crate::RepositoryObservationStore;

const ITEM4_MEMBERS: &str = include_str!("../tests/data/migration_history/item4_members_0018.sql");
const ITEM4_IDENTITY: &str =
    include_str!("../tests/data/migration_history/item4_identity_0018.sql");
const ITEM5_JOBS: &str = include_str!("../tests/data/migration_history/item5_jobs_0018.sql");

fn prefix(version: i64) -> Migrator {
    Migrator {
        migrations: Cow::Owned(
            MIGRATOR
                .iter()
                .filter(|m| m.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

async fn seed(sqlite: &SqliteConfig, migrator: &Migrator) {
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
        .await
        .unwrap();
    sqlite.run_migrations(&pool, migrator).await.unwrap();
    sqlx::query("INSERT INTO hierarchy_nodes VALUES ('root', 'project', NULL, 'project', NULL, '', NULL, NULL, NULL, 'active', 1, 1, 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO blackboard_entries VALUES ('legacy', 'project', 'root', 1, 1, 1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO blackboard_entry_revisions (entry_id, revision, kind, content, confidence_basis_points, verification, importance, root_promotion, state, provenance_kind, provenance_source_id, recorded_at_ms) VALUES ('legacy', 1, 'instruction', 'Preserve these exact legacy words: α.', 10000, 'unverified', 'high', 'promoted', 'tombstoned', 'user', 'source:legacy', 1)")
        .execute(&pool).await.unwrap();
    if migrator
        .iter()
        .any(|m| m.version == 18 && m.description == "capture members and scope revisions")
    {
        sqlx::query("INSERT INTO knowledge_context VALUES ('legacy', 1, 'project', 'rule', 'human_direct', NULL, NULL, 7, 0, 'group', 'historical', '{\"legacy\":true}')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO capture_groups VALUES ('project', 'group', 'thread', 'turn', 'rules', 1, 1, 1, 0, 0, 0, 0, 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO capture_group_members VALUES ('project', 'group', 0, 'legacy', 1, 'saved', 'Exact preview: α.', 'Exact reason: β.')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO memory_changes VALUES ('project', 7, 'legacy', 1, 'forgotten', 'direct_control', 'rule', 'forget-legacy', 'thread', 'turn', 'group', 'Exact preview: α.', 1)")
            .execute(&pool).await.unwrap();
    }
    pool.close().await;
}

async fn check_integrity(pool: &sqlx::SqlitePool) {
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(pool)
        .await
        .unwrap();
    let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .unwrap();
    assert_eq!((integrity, foreign_keys.len()), (vec!["ok".to_string()], 0));
}

#[tokio::test]
async fn migration_history_fresh_and_common_prefix_upgrade_reopen_preserves_legacy() {
    for version in [0, 14, 16, 18, 21] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        if version > 0 {
            seed(&sqlite, &prefix(version)).await;
        }
        let store = BlackboardStore::open(&sqlite).await.unwrap();
        let before = store
            .get_entry("project", &BlackboardEntryId::parse("legacy").unwrap())
            .await
            .unwrap();
        if version > 0 {
            let legacy = before.as_ref().unwrap();
            assert_eq!(
                (legacy.value.content.as_str(), legacy.state),
                (
                    "Preserve these exact legacy words: α.",
                    crate::BlackboardEntryState::Tombstoned
                )
            );
        }
        if version == 18 {
            let group = store
                .capture_group("project", "group")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                group.members,
                vec![crate::CaptureGroupMember {
                    ordinal: 0,
                    entry_id: Some("legacy".to_string()),
                    revision: Some(1),
                    outcome: MemberOutcome::Saved,
                    preview: "Exact preview: α.".to_string(),
                    reason: Some("Exact reason: β.".to_string()),
                }]
            );
            let context = store
                .knowledge_context("project", &BlackboardEntryId::parse("legacy").unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                context,
                crate::KnowledgeContext {
                    category: crate::KnowledgeCategory::Rule,
                    authority: crate::KnowledgeAuthority::HumanDirect,
                    scope_id: None,
                    end_condition: None,
                    source_sequence: Some(7),
                    unit_ordinal: Some(0),
                    group_id: Some("group".to_string()),
                    validity: crate::KnowledgeValidity::Historical,
                    payload: Some("{\"legacy\":true}".to_string()),
                }
            );
            sqlx::query("INSERT INTO capture_group_members VALUES ('project', 'group', 1, 'legacy', 1, 'not_restored', 'Retired', 'Do not restore')")
                .execute(&store.pool).await.unwrap();
        }
        check_integrity(&store.pool).await;
        let history: Vec<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&store.pool)
                .await
                .unwrap();
        store.pool.close().await;
        let store = BlackboardStore::open(&sqlite).await.unwrap();
        assert_eq!(
            store
                .get_entry("project", &BlackboardEntryId::parse("legacy").unwrap())
                .await
                .unwrap(),
            before
        );
        assert_eq!(
            sqlx::query_as::<_, (i64, Vec<u8>)>(
                "SELECT version, checksum FROM _sqlx_migrations ORDER BY version"
            )
            .fetch_all(&store.pool)
            .await
            .unwrap(),
            history
        );
        if version == 18 {
            assert_eq!(
                store
                    .capture_group("project", "group")
                    .await
                    .unwrap()
                    .unwrap()
                    .members[1]
                    .outcome,
                MemberOutcome::NotRestored
            );
            assert_eq!(store.latest_change_sequence("project").await.unwrap(), 7);
            store
                .record_change(
                    "project",
                    /*entry*/ None,
                    &crate::ChangeRecord {
                        operation: crate::ChangeOperation::Forgotten,
                        origin: crate::ChangeOrigin::DirectControl,
                        category: crate::KnowledgeCategory::Rule,
                        action_id: Some("next-action".to_string()),
                        thread_id: Some("thread".to_string()),
                        turn_id: None,
                        group_id: None,
                        preview: "Next change".to_string(),
                    },
                )
                .await
                .unwrap();
            assert_eq!(store.latest_change_sequence("project").await.unwrap(), 8);
        }
        check_integrity(&store.pool).await;
        store.pool.close().await;
    }
}

#[tokio::test]
async fn migration_history_incompatible_and_unknown_refused_by_every_pi_opener_unchanged() {
    for (description, sql) in [
        ("capture group members", ITEM4_MEMBERS),
        ("capture group members", ITEM4_IDENTITY),
        ("qualification jobs", ITEM5_JOBS),
        (
            "unknown history",
            "CREATE TABLE unknown_history (words TEXT);",
        ),
    ] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let mut migrator = prefix(/*version*/ 17);
        migrator.migrations.to_mut().push(Migration::new(
            /*version*/ 18,
            Cow::Borrowed(description),
            MigrationType::Simple,
            AssertSqlSafe(sql).into_sql_str(),
            /*no_tx*/ false,
        ));
        seed(&sqlite, &migrator).await;
        let path = sqlite.home().join(DATABASE_NAME);
        let before = Sha256::digest(std::fs::read(&path).unwrap());
        let checksum: String = migrator
            .iter()
            .last()
            .unwrap()
            .checksum
            .iter()
            .take(/*n*/ 12)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let expected = format!(
            "unsupported migration history: database={}, version=0018, description={description:?}, checksum={checksum}, success=true; original store untouched; automatic conversion is unavailable",
            path.display()
        );
        for error in [
            BlackboardStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
            HierarchyStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
            ContextMapStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
            RepositoryObservationStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
        ] {
            assert!(error.contains(&expected), "{error}");
            assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), before);
        }
        let pool = sqlite
            .open_read_only_pool(&path, /*busy_timeout*/ None)
            .await
            .unwrap();
        check_integrity(&pool).await;
        pool.close().await;
    }
}

#[tokio::test]
async fn migration_history_upgraded_action_retry_after_forget_preserves_journal_on_reopen() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    seed(&sqlite, &prefix(/*version*/ 18)).await;
    let store = BlackboardStore::open(&sqlite).await.unwrap();
    let id = BlackboardEntryId::parse("direct-after-upgrade").unwrap();
    let mut value = store
        .get_entry("project", &BlackboardEntryId::parse("legacy").unwrap())
        .await
        .unwrap()
        .unwrap()
        .value;
    value.content = "Exact direct words after upgrade: β.".to_string();
    let change = crate::ChangeRecord {
        operation: crate::ChangeOperation::Saved,
        origin: crate::ChangeOrigin::DirectControl,
        category: crate::KnowledgeCategory::Rule,
        action_id: Some("add-after-upgrade".to_string()),
        thread_id: Some("thread".to_string()),
        turn_id: None,
        group_id: Some("exact-fingerprint".to_string()),
        preview: value.content.clone(),
    };
    let request = || crate::CaptureWrite {
        project_id: "project".to_string(),
        units: vec![crate::CaptureEntryWrite {
            candidates: vec![id.clone()],
            value: value.clone(),
            context: crate::KnowledgeContext::new(
                crate::KnowledgeCategory::Rule,
                crate::KnowledgeAuthority::HumanDirect,
            ),
            change: change.clone(),
        }],
    };
    let entry = store
        .write_capture(request())
        .await
        .unwrap()
        .entries
        .remove(/*index*/ 0)
        .0;
    let forgotten = store
        .update_entry_recorded(
            "project",
            &id,
            crate::BlackboardEntryUpdate {
                expected_revision: entry.revision,
                kind: value.kind,
                content: value.content.clone(),
                structured_value: value.structured_value.clone(),
                confidence: value.confidence,
                verification: value.verification,
                importance: value.importance,
                root_promotion: value.root_promotion,
                evidence: value.evidence.clone(),
                premises: value.premises.clone(),
                state: crate::BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: value.provenance.clone(),
            },
            Some(&crate::ChangeRecord {
                operation: crate::ChangeOperation::Forgotten,
                action_id: Some("forget-after-upgrade".to_string()),
                ..change.clone()
            }),
        )
        .await
        .unwrap();
    let journal = store
        .memory_changes(
            "project", /*thread_id*/ None, /*after*/ 0, /*limit*/ 100,
        )
        .await
        .unwrap();
    let binding: (String, String, i64, String) = sqlx::query_as("SELECT request_fingerprint, entry_id, revision, outcome FROM capture_action_outcomes WHERE action_id = 'add-after-upgrade'")
        .fetch_one(&store.pool).await.unwrap();
    store.pool.close().await;
    let reopened = BlackboardStore::open(&sqlite).await.unwrap();
    assert!(matches!(
        reopened.write_capture(request()).await,
        Err(super::BlackboardStoreError::ActionAlreadyRecorded(_))
    ));
    assert_eq!(
        reopened.get_entry("project", &id).await.unwrap(),
        Some(forgotten)
    );
    assert_eq!(
        reopened
            .memory_changes(
                "project", /*thread_id*/ None, /*after*/ 0, /*limit*/ 100
            )
            .await
            .unwrap(),
        journal
    );
    assert_eq!(sqlx::query_as::<_, (String, String, i64, String)>("SELECT request_fingerprint, entry_id, revision, outcome FROM capture_action_outcomes WHERE action_id = 'add-after-upgrade'").fetch_one(&reopened.pool).await.unwrap(), binding);
    assert_eq!(reopened.latest_change_sequence("project").await.unwrap(), 9);
    let next = reopened
        .record_change(
            "project",
            /*entry*/ None,
            &crate::ChangeRecord {
                action_id: Some("next-after-reopen".to_string()),
                ..change
            },
        )
        .await
        .unwrap();
    assert_eq!(next, 10);
    check_integrity(&reopened.pool).await;
    reopened.pool.close().await;
}

#[tokio::test]
async fn migration_history_line_endings_supported_but_substantive_prefix_edit_refuses() {
    for (line_ending, changed) in [("\n", false), ("\r\n", false), ("\n", true)] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let mut historical = prefix(/*version*/ 14);
        let migration = historical.migrations.to_mut().last_mut().unwrap();
        let mut sql = migration
            .sql
            .as_str()
            .replace("\r\n", "\n")
            .replace('\n', line_ending);
        if changed {
            sql.push_str("\n-- substantive checksum variant\n");
        }
        *migration = Migration::new(
            migration.version,
            migration.description.clone(),
            migration.migration_type,
            AssertSqlSafe(sql).into_sql_str(),
            migration.no_tx,
        );
        seed(&sqlite, &historical).await;
        let path = sqlite.home().join(DATABASE_NAME);
        let before = Sha256::digest(std::fs::read(&path).unwrap());
        if changed {
            let error = BlackboardStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains("unsupported migration history") && error.contains("version=0014"),
                "{error}"
            );
            assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), before);
        } else {
            let store = BlackboardStore::open(&sqlite).await.unwrap();
            let applied: Vec<u8> =
                sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 14")
                    .fetch_one(&store.pool)
                    .await
                    .unwrap();
            assert_eq!(applied, historical.iter().last().unwrap().checksum.as_ref());
            check_integrity(&store.pool).await;
            store.pool.close().await;
        }
    }
}

#[tokio::test]
async fn migration_history_read_only_classification_handles_unicode_paths_and_writer_lock() {
    for incompatible in [false, true] {
        let home = TempDir::new().unwrap();
        let path = home.path().join("store – α");
        std::fs::create_dir(&path).unwrap();
        let sqlite = SqliteConfig::new_for_testing(path.abs());
        let mut historical = prefix(/*version*/ 18);
        if incompatible {
            historical.migrations.to_mut().pop();
            historical.migrations.to_mut().push(Migration::new(
                /*version*/ 18,
                Cow::Borrowed("capture group members"),
                MigrationType::Simple,
                AssertSqlSafe(ITEM4_MEMBERS).into_sql_str(),
                /*no_tx*/ false,
            ));
        }
        seed(&sqlite, &historical).await;
        let database = sqlite.home().join(DATABASE_NAME);
        let writer = sqlite.open_read_write_pool(&database).await.unwrap();
        let mut lock = writer.acquire().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *lock)
            .await
            .unwrap();
        let before = Sha256::digest(std::fs::read(&database).unwrap());
        let result = sqlite.check_migration_history(&database, &MIGRATOR).await;
        if incompatible {
            let error = result.unwrap_err().to_string();
            assert!(error.contains(&database.display().to_string()), "{error}");
            assert!(error.contains("unsupported migration history"), "{error}");
            // All openers must refuse before attempting a writer lock.
            assert!(BlackboardStore::open(&sqlite).await.is_err());
        } else {
            result.unwrap();
        }
        assert_eq!(Sha256::digest(std::fs::read(&database).unwrap()), before);
        sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();
        drop(lock);
        writer.close().await;
    }
}

#[tokio::test]
async fn migration_history_nonempty_untracked_database_refuses_every_opener_unchanged() {
    for empty_ledger in [false, true] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let path = sqlite.home().join(DATABASE_NAME);
        let pool = sqlite.open_read_write_pool(&path).await.unwrap();
        if empty_ledger {
            let empty = prefix(/*version*/ 0);
            sqlite.run_migrations(&pool, &empty).await.unwrap();
        }
        sqlx::query("CREATE TABLE legacy_words (words TEXT); INSERT INTO legacy_words VALUES ('Keep exact words: α.')").execute(&pool).await.unwrap();
        pool.close().await;
        let before = Sha256::digest(std::fs::read(&path).unwrap());
        for error in [
            BlackboardStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
            HierarchyStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
            ContextMapStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
            RepositoryObservationStore::open(&sqlite)
                .await
                .err()
                .unwrap()
                .to_string(),
        ] {
            for expected in [
                "unsupported migration history",
                "version=unknown",
                "missing or empty migration ledger",
                "checksum=unavailable",
            ] {
                assert!(error.contains(expected), "{error}");
            }
            assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), before);
        }
    }
}

#[tokio::test]
async fn migration_history_existing_empty_database_and_empty_ledger_migrate_normally() {
    for empty_ledger in [false, true] {
        let home = TempDir::new().unwrap();
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let path = sqlite.home().join(DATABASE_NAME);
        let pool = sqlite.open_read_write_pool(&path).await.unwrap();
        if empty_ledger {
            sqlite
                .run_migrations(&pool, &prefix(/*version*/ 0))
                .await
                .unwrap();
        }
        pool.close().await;
        let store = BlackboardStore::open(&sqlite).await.unwrap();
        let applied: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(applied, 22);
        check_integrity(&store.pool).await;
        store.pool.close().await;
    }
}

#[tokio::test]
async fn migration_history_ledger_name_view_is_not_treated_as_empty_database() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let path = sqlite.home().join(DATABASE_NAME);
    let pool = sqlite.open_read_write_pool(&path).await.unwrap();
    sqlx::query("CREATE VIEW _sqlx_migrations AS SELECT 'Unknown words: α.' AS words")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let before = Sha256::digest(std::fs::read(&path).unwrap());
    let error = BlackboardStore::open(&sqlite)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("unsupported migration history") && error.contains("version=unknown"),
        "{error}"
    );
    assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), before);
}
