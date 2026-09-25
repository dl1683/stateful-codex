use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::ProjectIndexFileRequest;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::EvidenceArguments;
use super::resolve_evidence;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";
const THREAD_ID: &str = "thread-1";

#[tokio::test]
async fn reindexed_source_cannot_certify_an_earlier_read() {
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
    let receipt_id = services.read_receipts().issue(
        PROJECT_ID,
        THREAD_ID,
        "read-call-1",
        BlackboardEvidenceLink {
            context_map_entry_id: context_hit.entry.id.clone(),
            source_fingerprint: context_hit.entry.value.source_fingerprint.clone(),
            line_range: Some(EvidenceLineRange { start: 1, end: 1 }),
        },
        b"threshold=10\n",
    );
    assert!(
        services
            .read_receipts()
            .resolve(PROJECT_ID, "another-thread", &receipt_id)
            .is_none()
    );
    std::fs::write(&source_path, "threshold=60\n").expect("change source");
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh_file(ProjectIndexFileRequest {
        project_id: PROJECT_ID.to_string(),
        project_root: project_root.path().to_path_buf(),
        relative_path: ProjectRelativePath::parse("policy.md").expect("relative path"),
    })
    .await
    .expect("refresh changed source");

    let result = resolve_evidence(
        PROJECT_ID,
        THREAD_ID,
        &services,
        &[project_root.path().to_path_buf()],
        vec![EvidenceArguments {
            read_receipt_id: Some(receipt_id),
            context_map_entry_id: None,
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
            .contains("blackboard evidence cited range changed after it was read")
    );
    assert!(error.to_string().contains("call evidence_read"));
}

#[tokio::test]
async fn reindexed_source_rebinds_a_receipt_when_the_cited_range_is_unchanged() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "# Policy\nthreshold=10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let indexer = ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    );
    indexer
        .refresh(ProjectIndexRequest {
            project_id: PROJECT_ID.to_string(),
            roots: vec![project_root.path().to_path_buf()],
        })
        .await
        .expect("index source");
    let original = services
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
    let line_range = EvidenceLineRange { start: 2, end: 2 };
    let receipt_id = services.read_receipts().issue(
        PROJECT_ID,
        THREAD_ID,
        "read-call-1",
        BlackboardEvidenceLink {
            context_map_entry_id: original.entry.id.clone(),
            source_fingerprint: original.entry.value.source_fingerprint,
            line_range: Some(line_range),
        },
        b"threshold=10\n",
    );
    std::fs::write(&source_path, "# Revised policy\nthreshold=10\n").expect("change heading");
    indexer
        .refresh_file(ProjectIndexFileRequest {
            project_id: PROJECT_ID.to_string(),
            project_root: project_root.path().to_path_buf(),
            relative_path: ProjectRelativePath::parse("policy.md").expect("relative path"),
        })
        .await
        .expect("refresh changed source");
    let current = services
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

    let (links, inferred_node_id) = resolve_evidence(
        PROJECT_ID,
        THREAD_ID,
        &services,
        &[project_root.path().to_path_buf()],
        vec![EvidenceArguments {
            read_receipt_id: Some(receipt_id),
            context_map_entry_id: None,
            relative_path: None,
            project_root: None,
            line_range: None,
        }],
    )
    .await
    .expect("unchanged cited bytes rebind to the current source");

    assert_eq!(
        (links, inferred_node_id),
        (
            vec![BlackboardEvidenceLink {
                context_map_entry_id: current.entry.id,
                source_fingerprint: current.entry.value.source_fingerprint,
                line_range: Some(line_range),
            }],
            Some(current.entry.value.node_id),
        )
    );
}
