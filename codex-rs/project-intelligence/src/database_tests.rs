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

fn read_only_error(operation: ProjectKnowledgeOperation) -> ProjectKnowledgeReadOnlyError {
    ProjectKnowledgeReadOnlyError {
        operation,
        project_id: "project-1".to_string(),
    }
}

fn assert_hierarchy_guard<T>(
    result: Result<T, HierarchyStoreError>,
    operation: ProjectKnowledgeOperation,
) {
    assert!(matches!(
        result,
        Err(HierarchyStoreError::ReadOnly(error)) if error == read_only_error(operation)
    ));
}

fn assert_context_map_guard<T>(
    result: Result<T, ContextMapStoreError>,
    operation: ProjectKnowledgeOperation,
) {
    assert!(matches!(
        result,
        Err(ContextMapStoreError::ReadOnly(error)) if error == read_only_error(operation)
    ));
}

fn assert_blackboard_guard<T>(
    result: Result<T, BlackboardStoreError>,
    operation: ProjectKnowledgeOperation,
) {
    assert!(matches!(
        result,
        Err(BlackboardStoreError::ReadOnly(error)) if error == read_only_error(operation)
    ));
}

fn assert_indexer_guard<T>(
    result: Result<T, ProjectIndexerError>,
    operation: ProjectKnowledgeOperation,
) {
    assert!(matches!(
        result,
        Err(ProjectIndexerError::ReadOnly(error)) if error == read_only_error(operation)
    ));
}

#[tokio::test]
async fn read_only_mutations_return_typed_errors_before_work() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let writable = seed_database(&sqlite).await;
    writable.pool.close().await;
    let database = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect("database should open read-only");
    let hierarchy = database.hierarchy_store();
    let context_map = database.context_map_store();
    let blackboard = database.blackboard_store();
    let indexer = database.indexer();
    let project_node = HierarchyNodeId::parse("node-project").expect("valid node ID");
    let entry_id = BlackboardEntryId::parse("entry-1").expect("valid entry ID");
    let project_root = PathBuf::from("C:\\project");
    let source_fingerprint = SourceFingerprint::parse("sha256:source").expect("valid fingerprint");
    let new_node = NewHierarchyNode {
        project_id: "project-1".to_string(),
        parent_id: None,
        kind: NodeKind::Project,
        project_root: None,
        relative_path: ProjectRelativePath::root(),
        region_anchor: None,
        source_fingerprint: None,
    };
    assert_hierarchy_guard(
        hierarchy.create_node(project_node.clone(), new_node).await,
        ProjectKnowledgeOperation::HierarchyCreateNode,
    );
    assert_hierarchy_guard(
        hierarchy
            .update_source_state(
                "project-1",
                &project_node,
                HierarchySourceUpdate {
                    expected_revision: 0,
                    lifecycle: NodeLifecycle::Active,
                    source_fingerprint: None,
                },
            )
            .await,
        ProjectKnowledgeOperation::HierarchyUpdateSourceState,
    );
    assert_hierarchy_guard(
        hierarchy
            .update_region_source(
                "project-1",
                &project_node,
                HierarchyRegionSourceUpdate {
                    expected_revision: 0,
                    lifecycle: NodeLifecycle::Active,
                    region_anchor: RegionAnchor::new("lines", "1-1").expect("valid anchor"),
                    source_fingerprint: source_fingerprint.clone(),
                },
            )
            .await,
        ProjectKnowledgeOperation::HierarchyUpdateRegionSource,
    );
    assert_context_map_guard(
        context_map
            .create_entry(
                ContextMapEntryId::parse("map-1").expect("valid map ID"),
                NewContextMapEntry {
                    project_id: "project-1".to_string(),
                    node_id: project_node.clone(),
                    source_fingerprint: source_fingerprint.clone(),
                    description: "description".to_string(),
                    routing_terms: vec!["term".to_string()],
                    coverage: ContextMapCoverage::Complete,
                },
            )
            .await,
        ProjectKnowledgeOperation::ContextMapCreateEntry,
    );
    assert_context_map_guard(
        context_map
            .update_entry(
                "project-1",
                &ContextMapEntryId::parse("map-1").expect("valid map ID"),
                ContextMapEntryUpdate {
                    expected_revision: 0,
                    source_fingerprint: source_fingerprint.clone(),
                    description: "description".to_string(),
                    routing_terms: vec!["term".to_string()],
                    coverage: ContextMapCoverage::Complete,
                },
            )
            .await,
        ProjectKnowledgeOperation::ContextMapUpdateEntry,
    );
    let new_entry = NewBlackboardEntry {
        project_id: "project-1".to_string(),
        node_id: project_node.clone(),
        kind: BlackboardKind::Fact,
        content: "content".to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(5_000).expect("valid confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::Normal,
        root_promotion: RootPromotion::NotPromoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "test".to_string(),
        },
    };
    assert_blackboard_guard(
        blackboard
            .create_entry(entry_id.clone(), new_entry.clone())
            .await,
        ProjectKnowledgeOperation::BlackboardCreateEntry,
    );
    assert_blackboard_guard(
        blackboard
            .update_entry(
                "project-1",
                &entry_id,
                BlackboardEntryUpdate {
                    expected_revision: 0,
                    kind: new_entry.kind,
                    content: new_entry.content.clone(),
                    structured_value: None,
                    confidence: new_entry.confidence,
                    verification: new_entry.verification,
                    importance: new_entry.importance,
                    root_promotion: new_entry.root_promotion,
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    state: BlackboardEntryState::Active,
                    superseded_by: None,
                    provenance: new_entry.provenance.clone(),
                },
            )
            .await,
        ProjectKnowledgeOperation::BlackboardUpdateEntry,
    );
    assert_blackboard_guard(
        blackboard
            .create_relation(
                BlackboardRelationId::parse("relation-1").expect("valid relation ID"),
                NewBlackboardRelation {
                    project_id: "project-1".to_string(),
                    from_entry_id: entry_id.clone(),
                    to_entry_id: BlackboardEntryId::parse("entry-2").expect("valid entry ID"),
                    kind: BlackboardRelationKind::RelatedTo,
                    note: None,
                    confidence: new_entry.confidence,
                    provenance: new_entry.provenance.clone(),
                },
            )
            .await,
        ProjectKnowledgeOperation::BlackboardCreateRelation,
    );
    assert!(matches!(
        blackboard
            .acquire_completion_fence("project-1", Duration::from_millis(10))
            .await,
        Err(BlackboardStoreError::ReadOnly(error))
            if error == read_only_error(ProjectKnowledgeOperation::BlackboardAcquireCompletionFence)
    ));
    assert_indexer_guard(
        indexer
            .refresh(ProjectIndexRequest {
                project_id: "project-1".to_string(),
                roots: vec![project_root.clone()],
            })
            .await,
        ProjectKnowledgeOperation::ContextMapRefresh,
    );
    assert_indexer_guard(
        indexer
            .refresh_file(ProjectIndexFileRequest {
                project_id: "project-1".to_string(),
                project_root,
                relative_path: ProjectRelativePath::parse("missing.md").expect("valid path"),
            })
            .await,
        ProjectKnowledgeOperation::ContextMapRefreshFile,
    );
}

#[tokio::test]
async fn read_only_queries_do_not_change_database_files_or_sidecars() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = sqlite_config(&temp_dir);
    let writable = seed_database(&sqlite).await;
    writable.pool.close().await;
    let path = sqlite.home().join(DATABASE_NAME);
    let before = database_files(sqlite.home());
    let database = ProjectKnowledgeDatabase::open(&sqlite, ProjectKnowledgeAccess::ReadOnly)
        .await
        .expect("database should open read-only");
    let hierarchy = database.hierarchy_store();
    let context_map = database.context_map_store();
    let blackboard = database.blackboard_store();
    assert_eq!(
        hierarchy
            .project_node("project-1")
            .await
            .expect("hierarchy read should succeed"),
        None
    );
    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "query".to_string(),
                max_results: 1,
            })
            .await
            .expect("context-map read should succeed")
            .data,
        Vec::new()
    );
    assert_eq!(
        blackboard
            .query(BlackboardQuery {
                project_id: "project-1".to_string(),
                text: None,
                within_node: None,
                root_promotion: None,
                entry_scope: BlackboardEntryScope::Active,
                max_results: 1,
            })
            .await
            .expect("blackboard read should succeed")
            .data,
        Vec::new()
    );
    assert_eq!(
        blackboard
            .root_projection(RootBlackboardQuery {
                project_id: "project-1".to_string(),
                max_entries: 1,
            })
            .await
            .expect("root projection should succeed")
            .data,
        Vec::new()
    );
    assert_eq!(
        hierarchy
            .project_intelligence_status("project-1")
            .await
            .expect("status read should succeed")
            .initialized,
        false
    );
    drop((hierarchy, context_map, blackboard, database));
    let after = database_files(sqlite.home());
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        sha2::Sha256::digest(&before[&DATABASE_NAME.to_string()]),
        sha2::Sha256::digest(&after[&DATABASE_NAME.to_string()])
    );
    assert!(path.is_file());
}

fn database_files(home: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(home)
        .expect("database home should be readable")
        .map(|entry| {
            let entry = entry.expect("directory entry should read");
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).expect("database file should read"),
            )
        })
        .collect()
}
