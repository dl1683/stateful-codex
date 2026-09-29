use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sqlx::AssertSqlSafe;
use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

use super::*;
use crate::BlackboardEntryId;
use crate::BlackboardEntryScope;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardQuery;
use crate::BlackboardRelationId;
use crate::BlackboardRelationKind;
use crate::BlackboardStoreError;
use crate::BlackboardVerification;
use crate::ConfidenceScore;
use crate::ContextMapCoverage;
use crate::ContextMapEntryId;
use crate::ContextMapEntryUpdate;
use crate::ContextMapQuery;
use crate::ContextMapStoreError;
use crate::HierarchyNodeId;
use crate::HierarchyRegionSourceUpdate;
use crate::HierarchySourceUpdate;
use crate::HierarchyStoreError;
use crate::NewBlackboardEntry;
use crate::NewBlackboardRelation;
use crate::NewContextMapEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectIndexFileRequest;
use crate::ProjectIndexRequest;
use crate::ProjectIndexerError;
use crate::ProjectRelativePath;
use crate::RegionAnchor;
use crate::RootBlackboardQuery;
use crate::RootPromotion;
use crate::SourceFingerprint;

fn sqlite_config(temp_dir: &TempDir) -> SqliteConfig {
    SqliteConfig::new_for_testing(temp_dir.path().abs())
}

async fn seed_database(sqlite: &SqliteConfig) -> ProjectKnowledgeDatabase {
    ProjectKnowledgeDatabase::open(sqlite, ProjectKnowledgeAccess::ReadWrite)
        .await
        .expect("database should migrate")
}

async fn edit_migrations<F>(sqlite: &SqliteConfig, edit: F)
where
    F: FnOnce(
        &mut sqlx::SqliteConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>,
{
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
        .await
        .expect("migrated database should open");
    let mut connection = pool.acquire().await.expect("connection should acquire");
    edit(&mut connection).await;
    drop(connection);
    pool.close().await;
}

#[tokio::test]
async fn read_only_open_rejects_missing_database_without_creating_parent() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().join("missing").abs());
    let path = sqlite.home().join(DATABASE_NAME);

    let error = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect_err("missing database should be rejected");

    assert_eq!(
        error.to_string(),
        format!(
            "project intelligence database is missing: {}",
            path.display()
        )
    );
    assert!(!sqlite.home().exists());
}

#[tokio::test]
async fn read_only_open_rejects_database_without_migration_table() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    tokio::fs::create_dir_all(sqlite.home())
        .await
        .expect("database directory created");
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
        .await
        .expect("empty database should open");
    pool.close().await;

    let error = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect_err("unmigrated database should be rejected");
    assert_eq!(
        error.to_string(),
        format!(
            "project intelligence database is missing migration 1: {}",
            sqlite.home().join(DATABASE_NAME).display()
        )
    );
}

#[tokio::test]
async fn read_only_open_rejects_partially_migrated_database() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let database = seed_database(&sqlite).await;
    database.pool.close().await;
    edit_migrations(&sqlite, |connection| {
        Box::pin(async move {
            sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 14")
                .execute(&mut *connection)
                .await
                .expect("migration row should delete");
        })
    })
    .await;

    let error = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect_err("partial migration should be rejected");
    assert!(matches!(
        error,
        ProjectKnowledgeDatabaseError::UnmigratedDatabase {
            missing_version: 14,
            ..
        }
    ));
}

#[tokio::test]
async fn read_only_open_rejects_failed_migration() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let database = seed_database(&sqlite).await;
    database.pool.close().await;
    edit_migrations(&sqlite, |connection| {
        Box::pin(async move {
            sqlx::query("UPDATE _sqlx_migrations SET success = 0 WHERE version = 14")
                .execute(&mut *connection)
                .await
                .expect("migration row should update");
        })
    })
    .await;

    let error = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect_err("failed migration should be rejected");
    assert!(matches!(
        error,
        ProjectKnowledgeDatabaseError::FailedMigration { version: 14, .. }
    ));
}

#[tokio::test]
async fn read_only_open_rejects_checksum_mismatch() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let database = seed_database(&sqlite).await;
    database.pool.close().await;
    edit_migrations(&sqlite, |connection| {
        Box::pin(async move {
            sqlx::query("UPDATE _sqlx_migrations SET checksum = zeroblob(32) WHERE version = 1")
                .execute(&mut *connection)
                .await
                .expect("migration checksum should update");
        })
    })
    .await;

    let error = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect_err("checksum mismatch should be rejected");
    assert!(matches!(
        error,
        ProjectKnowledgeDatabaseError::MigrationChecksumMismatch { version: 1, .. }
    ));
}

#[tokio::test]
async fn read_only_open_rejects_unknown_newer_migration() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let database = seed_database(&sqlite).await;
    database.pool.close().await;
    edit_migrations(&sqlite, |connection| {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO _sqlx_migrations
                 (version, description, installed_on, success, checksum, execution_time)
                 SELECT 15, 'future', installed_on, success, checksum, execution_time
                 FROM _sqlx_migrations WHERE version = 14",
            )
            .execute(&mut *connection)
            .await
            .expect("future migration row should insert");
        })
    })
    .await;

    let error = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect_err("unknown migration should be rejected");
    assert!(matches!(
        error,
        ProjectKnowledgeDatabaseError::UnknownMigration { version: 15, .. }
    ));
}

#[tokio::test]
async fn read_only_open_accepts_line_ending_equivalent_checksums() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let database = seed_database(&sqlite).await;
    database.pool.close().await;
    let migration = MIGRATOR.migrations.first().expect("migration 1 exists");
    let crlf_sql = migration
        .sql
        .as_str()
        .replace("\r\n", "\n")
        .replace('\n', "\r\n");
    let crlf_checksum = Migration::new(
        migration.version,
        migration.description.clone(),
        migration.migration_type,
        AssertSqlSafe(crlf_sql).into_sql_str(),
        migration.no_tx,
    )
    .checksum;
    edit_migrations(&sqlite, move |connection| {
        Box::pin(async move {
            sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
                .bind(crlf_checksum.as_ref())
                .execute(&mut *connection)
                .await
                .expect("line-ending checksum should update");
        })
    })
    .await;

    let database = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect("line-ending equivalent checksum should be accepted");
    assert_eq!(database.access(), ProjectKnowledgeAccess::ReadOnly);
    assert_eq!(database.path(), sqlite.home().join(DATABASE_NAME).as_path());
    database.pool.close().await;
}

#[tokio::test]
async fn read_only_open_accepts_complete_existing_database() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let database = seed_database(&sqlite).await;
    database.pool.close().await;

    let database = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect("complete database should open read-only");
    assert_eq!(database.access(), ProjectKnowledgeAccess::ReadOnly);
    database.pool.close().await;
}

