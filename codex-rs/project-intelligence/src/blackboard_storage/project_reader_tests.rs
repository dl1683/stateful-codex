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

#[expect(
    clippy::disallowed_methods,
    reason = "explicit disposable project fixture"
)]
async fn fixture(home: &TempDir) -> (ProjectKnowledgeReader, StoreIdentity, SqliteConnection) {
    let location = ProjectStoreLocation::select_project(&home.path().abs()).unwrap();
    let identity = initialize_local_store(&location).await.unwrap();
    let manifest = [DomainManifest {
        domain: "project-intelligence",
        migrations: &super::super::MIGRATOR,
    }];
    let domains = PreparedDomains::prepare(&manifest).await.unwrap();
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .foreign_keys(/*on*/ true),
    )
    .await
    .unwrap();
    domains.install(&mut writer).await.unwrap();
    let reader = ProjectStoreReader::open(&location, domains).await.unwrap();
    (
        ProjectKnowledgeReader::from_project_store(reader)
            .await
            .unwrap(),
        identity,
        writer,
    )
}

#[tokio::test]
async fn metadata_is_bound_fresh_lossless_and_reusable_after_ordinary_refusal() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    let expected = ProjectKnowledgeSnapshot {
        identity: identity.clone(),
        project_revision: 0,
        eligibility_generation: 0,
        coverage_complete: true,
    };
    assert_eq!(reader.inspect(&identity).await.unwrap(), expected);
    let other = TempDir::new().unwrap();
    let other_identity =
        initialize_local_store(&ProjectStoreLocation::select_project(&other.path().abs()).unwrap())
            .await
            .unwrap();
    assert!(matches!(
        reader.inspect(&other_identity).await,
        Err(ProjectKnowledgeReadError::IdentityMismatch)
    ));
    let mut wrong = identity.clone();
    wrong.project_uuid = other_identity.project_uuid;
    assert!(matches!(
        reader.inspect(&wrong).await,
        Err(ProjectKnowledgeReadError::IdentityMismatch)
    ));
    wrong = identity.clone();
    wrong.store_uuid = other_identity.store_uuid;
    assert!(matches!(
        reader.inspect(&wrong).await,
        Err(ProjectKnowledgeReadError::IdentityMismatch)
    ));
    assert_eq!(reader.inspect(&identity).await.unwrap(), expected);
    sqlx::query("INSERT INTO project_intelligence_revisions VALUES (?,9007199254740993)")
        .bind(identity.project_uuid.to_string())
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_identity_coverage(project_id,watermark) VALUES (?,0)")
        .bind(identity.project_uuid.to_string())
        .execute(&mut writer)
        .await
        .unwrap();
    let changed = ProjectKnowledgeSnapshot {
        project_revision: 9_007_199_254_740_993,
        eligibility_generation: 1,
        coverage_complete: false,
        ..expected
    };
    assert_eq!(reader.inspect(&identity).await.unwrap(), changed);
    sqlx::query("UPDATE project_store_identity SET format_version=3")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.inspect(&wrong).await,
        Err(ProjectKnowledgeReadError::IdentityMismatch)
    ));
    sqlx::query("UPDATE project_store_identity SET format_version=2")
        .execute(&mut writer)
        .await
        .unwrap();
    assert_eq!(reader.inspect(&identity).await.unwrap(), changed);
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

#[tokio::test]
async fn malformed_clocks_refuse_without_coercion_and_before_foreign_ownership() {
    for (table, value) in [
        ("project_intelligence_revisions", "'broken'"),
        ("project_intelligence_revisions", "1.5"),
        ("project_intelligence_revisions", "x'01'"),
        ("project_intelligence_revisions", "-1"),
        ("capsule_eligibility_generations", "'broken'"),
        ("capsule_eligibility_generations", "1.5"),
        ("capsule_eligibility_generations", "0"),
    ] {
        let home = TempDir::new().unwrap();
        let (mut reader, identity, mut writer) = fixture(&home).await;
        // -1 needs bypassing the legacy CHECK only in this corruption fixture.
        sqlx::query("PRAGMA ignore_check_constraints=ON")
            .execute(&mut writer)
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {table} VALUES ('foreign',{value})"
        )))
        .execute(&mut writer)
        .await
        .unwrap();
        assert!(matches!(
            reader.inspect(&identity).await,
            Err(ProjectKnowledgeReadError::InvalidClock)
        ));
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .execute(&mut writer)
            .await
            .unwrap();
        assert_eq!(
            reader.inspect(&identity).await.unwrap(),
            ProjectKnowledgeSnapshot {
                identity: identity.clone(),
                project_revision: 0,
                eligibility_generation: 0,
                coverage_complete: true,
            }
        );
        reader.close().await.unwrap();
        writer.close().await.unwrap();
    }
}

#[tokio::test]
#[expect(
    clippy::disallowed_methods,
    reason = "explicit disposable manifest mismatch fixture"
)]
async fn same_schema_admitted_under_another_domain_cannot_construct_knowledge_reader() {
    let home = TempDir::new().unwrap();
    let location = ProjectStoreLocation::select_project(&home.path().abs()).unwrap();
    initialize_local_store(&location).await.unwrap();
    let domains = PreparedDomains::prepare(&[DomainManifest {
        domain: "other-domain",
        migrations: &super::super::MIGRATOR,
    }])
    .await
    .unwrap();
    let mut writer = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(location.database_path().as_path())
            .foreign_keys(/*on*/ true),
    )
    .await
    .unwrap();
    domains.install(&mut writer).await.unwrap();
    let reader = ProjectStoreReader::open(&location, domains).await.unwrap();
    assert!(matches!(
        ProjectKnowledgeReader::from_project_store(reader).await,
        Err(ProjectKnowledgeReadError::Registry(
            StoreRegistryError::IncompatibleHistory
        ))
    ));
    writer.close().await.unwrap();
    let moved_parent = TempDir::new().unwrap();
    std::fs::rename(
        home.path().join(".stateful"),
        moved_parent.path().join(".stateful"),
    )
    .unwrap();
}

#[tokio::test]
async fn ownership_inventory_tracks_schema_and_refuses_foreign_derived_state() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    let actual: Vec<String> = sqlx::query_scalar(
        "SELECT object.name FROM sqlite_schema AS object WHERE object.type='table' AND EXISTS(SELECT 1 FROM pragma_table_info(object.name) AS field WHERE field.name='project_id') ORDER BY object.name",
    ).fetch_all(&mut writer).await.unwrap();
    let mut expected: Vec<String> = ownership::PROJECT_TABLES
        .iter()
        .map(|table| (*table).to_string())
        .collect();
    expected.sort();
    assert_eq!(actual, expected);
    sqlx::query("INSERT INTO context_map_search(entry_id,project_id,description,routing_terms) VALUES ('missing',?,'omitted','omitted')")
        .bind(identity.project_uuid.to_string()).execute(&mut writer).await.unwrap();
    assert!(matches!(
        reader.inspect(&identity).await,
        Err(ProjectKnowledgeReadError::InvalidOwnership)
    ));
    sqlx::query("DELETE FROM context_map_search")
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_projection_coverage(project_id) VALUES ('foreign')")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.inspect(&identity).await,
        Err(ProjectKnowledgeReadError::InvalidOwnership)
    ));
    reader.close().await.unwrap();
    writer.close().await.unwrap();
}

#[tokio::test]
async fn orphan_links_and_missing_current_revision_are_distinct_and_close_releases_file() {
    let home = TempDir::new().unwrap();
    let (mut reader, identity, mut writer) = fixture(&home).await;
    sqlx::query("PRAGMA foreign_keys=OFF")
        .execute(&mut writer)
        .await
        .unwrap();
    sqlx::query("INSERT INTO capture_entry_sources VALUES ('orphan','missing',0,1,'quote')")
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.inspect(&identity).await,
        Err(ProjectKnowledgeReadError::InvalidOwnership)
    ));
    sqlx::query("INSERT INTO blackboard_entries VALUES ('entry',?,'missing-node',1,0,0)")
        .bind(identity.project_uuid.to_string())
        .execute(&mut writer)
        .await
        .unwrap();
    assert!(matches!(
        reader.inspect(&identity).await,
        Err(ProjectKnowledgeReadError::InvalidEntry)
    ));
    reader.close().await.unwrap();
    writer.close().await.unwrap();
    let moved_parent = TempDir::new().unwrap();
    std::fs::rename(
        home.path().join(".stateful"),
        moved_parent.path().join(".stateful"),
    )
    .unwrap();
}

#[path = "project_reader_content_tests.rs"]
mod content_tests;
