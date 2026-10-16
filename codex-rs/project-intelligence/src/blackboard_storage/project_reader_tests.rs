use codex_project_store::DomainManifest;
use codex_project_store::PreparedDomains;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreReader;
use codex_project_store::initialize_local_store;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::Connection;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;
use tempfile::TempDir;

use super::*;

#[expect(clippy::disallowed_methods, reason = "explicit disposable project fixture")]
async fn fixture(home: &TempDir) -> (ProjectKnowledgeReader, StoreIdentity, SqliteConnection) {
    let location = ProjectStoreLocation::select_project(&home.path().abs()).unwrap();
    let identity = initialize_local_store(&location).await.unwrap();
    let manifest = [DomainManifest {
        domain: "project-intelligence",
        migrations: &super::super::MIGRATOR,
    }];
    let domains = PreparedDomains::prepare(&manifest).await.unwrap();
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(location.database_path().as_path()).foreign_keys(true),
    ).await.unwrap();
    domains.install(&mut writer).await.unwrap();
    let reader = ProjectStoreReader::open(&location, domains).await.unwrap();
    (ProjectKnowledgeReader::from_project_store(reader).await.unwrap(), identity, writer)
}

#[tokio::test]
async fn metadata_is_bound_fresh_lossless_and_reusable_after_ordinary_refusal() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    let expected = ProjectKnowledgeSnapshot {
        identity: identity.clone(), project_revision: 0, eligibility_generation: 0, coverage_complete: true,
    };
    assert_eq!(reader.inspect(&identity).await.unwrap(), expected);
    let other = TempDir::new().unwrap();
    let other_identity = initialize_local_store(&ProjectStoreLocation::select_project(&other.path().abs()).unwrap()).await.unwrap();
    assert!(matches!(reader.inspect(&other_identity).await, Err(ProjectKnowledgeReadError::IdentityMismatch)));
    assert_eq!(reader.inspect(&identity).await.unwrap(), expected);
    sqlx::query("INSERT INTO project_intelligence_revisions VALUES (?,9007199254740993)")
        .bind(identity.project_uuid.to_string()).execute(&mut writer).await.unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id,watermark) VALUES (?,0)")
        .bind(identity.project_uuid.to_string()).execute(&mut writer).await.unwrap();
    assert_eq!(reader.inspect(&identity).await.unwrap(), ProjectKnowledgeSnapshot {
        project_revision: 9_007_199_254_740_993, eligibility_generation: 1, coverage_complete: false, ..expected
    });
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

#[tokio::test]
async fn malformed_clocks_refuse_without_coercion_and_before_foreign_ownership() {
    for value in ["'broken'", "1.5", "x'01'", "-1"] {
        let home = TempDir::new().unwrap();
        let (mut reader, identity, mut writer) = fixture(&home).await;
        // -1 needs bypassing the legacy CHECK only in this corruption fixture.
        sqlx::query("PRAGMA ignore_check_constraints=ON").execute(&mut writer).await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO project_intelligence_revisions VALUES ('foreign',{value})")))
            .execute(&mut writer).await.unwrap();
        assert!(matches!(reader.inspect(&identity).await, Err(ProjectKnowledgeReadError::InvalidClock)));
        sqlx::query("DELETE FROM project_intelligence_revisions").execute(&mut writer).await.unwrap();
        assert_eq!(reader.inspect(&identity).await.unwrap(), ProjectKnowledgeSnapshot {
            identity: identity.clone(), project_revision: 0, eligibility_generation: 0, coverage_complete: true,
        });
        reader.close().await.unwrap();
        writer.close().await.unwrap();
    }
}

#[tokio::test]
async fn ownership_inventory_tracks_schema_and_refuses_foreign_derived_state() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    let actual: Vec<String> = sqlx::query_scalar(
        "SELECT object.name FROM sqlite_schema AS object WHERE object.type='table' AND EXISTS(SELECT 1 FROM pragma_table_info(object.name) AS field WHERE field.name='project_id') ORDER BY object.name",
    ).fetch_all(&mut writer).await.unwrap();
    let mut expected: Vec<String> = ownership::PROJECT_TABLES.iter().map(|table| (*table).to_string()).collect();
    expected.sort();
    assert_eq!(actual, expected);
    sqlx::query("INSERT INTO capture_projection_coverage(project_id) VALUES ('foreign')").execute(&mut writer).await.unwrap();
    assert!(matches!(reader.inspect(&identity).await, Err(ProjectKnowledgeReadError::InvalidOwnership)));
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

#[tokio::test]
async fn orphan_links_and_missing_current_revision_are_distinct_and_close_releases_file() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    sqlx::query("PRAGMA foreign_keys=OFF").execute(&mut writer).await.unwrap();
    sqlx::query("INSERT INTO capture_entry_sources VALUES ('orphan','missing',0,1,'quote')")
        .execute(&mut writer).await.unwrap();
    assert!(matches!(reader.inspect(&identity).await, Err(ProjectKnowledgeReadError::InvalidOwnership)));
    sqlx::query("INSERT INTO blackboard_entries VALUES ('entry',?,'missing-node',1,0,0)")
        .bind(identity.project_uuid.to_string()).execute(&mut writer).await.unwrap();
    assert!(matches!(reader.inspect(&identity).await, Err(ProjectKnowledgeReadError::InvalidEntry)));
    reader.close().await.unwrap();
    writer.close().await.unwrap();
    let moved_parent = TempDir::new().unwrap();
    std::fs::rename(home.path().join(".stateful"), moved_parent.path().join(".stateful")).unwrap();
}
