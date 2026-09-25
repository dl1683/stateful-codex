use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::HierarchySourceUpdate;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NodeLifecycle;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::SourceFingerprint;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::SourceAuditStatus;
use super::SourceCheck;
use super::reconcile_source_state;

#[tokio::test]
async fn stale_audit_observation_cannot_regress_a_concurrent_refresh() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    std::fs::write(project_root.path().join("policy.md"), "threshold=10\n").expect("write source");
    let sqlite = SqliteConfig::new_for_testing(state_home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    ProjectIndexer::new(hierarchy.clone(), context_map.clone())
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![project_root.path().to_path_buf()],
        })
        .await
        .expect("index source");
    let hit = context_map
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let observed_node = hierarchy
        .get_node("project-1", &hit.entry.value.node_id)
        .await
        .expect("node lookup")
        .expect("source node");
    let digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let refreshed_fingerprint =
        SourceFingerprint::parse(format!("sha256:{digest}")).expect("fingerprint");
    let refreshed_node = hierarchy
        .update_source_state(
            "project-1",
            &observed_node.id,
            HierarchySourceUpdate {
                expected_revision: observed_node.revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(refreshed_fingerprint.clone()),
            },
        )
        .await
        .expect("simulate concurrent refresh");

    let audited_bytes = 13;
    let status = reconcile_source_state(
        "project-1",
        &hierarchy,
        &hit,
        &observed_node,
        SourceCheck::Fingerprint(refreshed_fingerprint, audited_bytes),
    )
    .await;

    assert_eq!(status, SourceAuditStatus::Stale);
    let current_node = hierarchy
        .get_node("project-1", &hit.entry.value.node_id)
        .await
        .expect("node lookup")
        .expect("source node");
    assert_eq!(current_node, refreshed_node);
}
