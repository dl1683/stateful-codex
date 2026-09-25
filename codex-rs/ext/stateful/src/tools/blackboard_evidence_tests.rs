use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use tempfile::TempDir;

use super::EvidenceArguments;
use super::resolve_evidence;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";

#[tokio::test]
async fn changed_source_cannot_be_persisted_as_current_evidence() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "threshold=10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: PROJECT_ID.to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    let context_hit = services
        .context_map()
        .await
        .expect("context map")
        .file_hits_for_path(
            PROJECT_ID,
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    std::fs::write(&source_path, "threshold=60\n").expect("change source");

    let result = resolve_evidence(
        PROJECT_ID,
        &services,
        &[project_root.path().to_path_buf()],
        vec![EvidenceArguments {
            context_map_entry_id: Some(context_hit.entry.id.to_string()),
            relative_path: None,
            project_root: None,
            line_range: None,
        }],
    )
    .await;
    let Err(error) = result else {
        panic!("changed evidence must be rejected before persistence");
    };

    assert!(
        error
            .to_string()
            .contains("blackboard evidence source changed")
    );
    assert!(error.to_string().contains("call evidence_read"));
}
