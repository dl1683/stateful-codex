use std::sync::Arc;

use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::EvidenceReadTool;
use crate::services::ProjectIntelligenceServices;

#[tokio::test]
async fn changed_source_is_incrementally_refreshed_and_reread_once() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "# Policy\nThreshold: 10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    std::fs::write(&source_path, "# Policy\nThreshold: 60\n").expect("change source");
    let tool = EvidenceReadTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
    );
    let relative_path = ProjectRelativePath::parse("policy.md").expect("relative path");

    let (refreshed, source_refreshed) = tool
        .read_with_refresh(
            vec![project_root.path().to_path_buf()],
            None,
            relative_path.clone(),
            Some(EvidenceLineRange { start: 2, end: 2 }),
            1024,
        )
        .await
        .expect("refresh and reread source");
    assert!(source_refreshed);
    assert_eq!(refreshed.content, "Threshold: 60\n");

    let (unchanged, source_refreshed) = tool
        .read_with_refresh(
            vec![project_root.path().to_path_buf()],
            None,
            relative_path,
            Some(EvidenceLineRange { start: 2, end: 2 }),
            1024,
        )
        .await
        .expect("reuse refreshed route");
    assert!(!source_refreshed);
    assert_eq!(unchanged.content, "Threshold: 60\n");
}
