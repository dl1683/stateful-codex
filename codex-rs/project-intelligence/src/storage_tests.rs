use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

fn project_node() -> NewHierarchyNode {
    NewHierarchyNode {
        project_id: "project-1".to_string(),
        parent_id: None,
        kind: NodeKind::Project,
        project_root: None,
        relative_path: ProjectRelativePath::root(),
        region_anchor: None,
        source_fingerprint: None,
    }
}

fn child_node(
    parent_id: &HierarchyNodeId,
    kind: NodeKind,
    relative_path: &str,
) -> NewHierarchyNode {
    NewHierarchyNode {
        project_id: "project-1".to_string(),
        parent_id: Some(parent_id.clone()),
        kind,
        project_root: Some("C:\\workspace".to_string()),
        relative_path: ProjectRelativePath::parse(relative_path).expect("valid path"),
        region_anchor: None,
        source_fingerprint: Some("sha256:abc".to_string()),
    }
}

async fn open_store(temp_dir: &TempDir) -> HierarchyStore {
    HierarchyStore::open(&SqliteConfig::new_for_testing(temp_dir.path().abs()))
        .await
        .expect("store should open")
}

#[tokio::test]
async fn hierarchy_survives_reopen_and_lists_direct_children_in_stable_order() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let store = open_store(&temp_dir).await;
    let project_id = HierarchyNodeId::parse("node-project").expect("valid ID");
    let root_id = HierarchyNodeId::parse("node-root").expect("valid ID");
    let docs_id = HierarchyNodeId::parse("node-docs").expect("valid ID");
    let src_id = HierarchyNodeId::parse("node-src").expect("valid ID");
    let file_id = HierarchyNodeId::parse("node-file").expect("valid ID");
    let region_id = HierarchyNodeId::parse("node-region").expect("valid ID");

    let project = store
        .create_node(project_id.clone(), project_node())
        .await
        .expect("project node should insert");
    let root = store
        .create_node(
            root_id.clone(),
            child_node(&project_id, NodeKind::Directory, ""),
        )
        .await
        .expect("root directory should insert");
    let docs = store
        .create_node(docs_id, child_node(&root_id, NodeKind::Directory, "docs"))
        .await
        .expect("directory should insert");
    let src = store
        .create_node(
            src_id.clone(),
            child_node(&root_id, NodeKind::Directory, "src"),
        )
        .await
        .expect("directory should insert");
    let file = store
        .create_node(
            file_id.clone(),
            child_node(&src_id, NodeKind::File, "src/lib.rs"),
        )
        .await
        .expect("file should insert");
    let mut region_value = child_node(&file_id, NodeKind::Region, "src/lib.rs");
    region_value.region_anchor =
        Some(RegionAnchor::new("symbol", "crate::open").expect("valid anchor"));
    let region = store
        .create_node(region_id.clone(), region_value)
        .await
        .expect("region should insert");

    drop(store);
    let reopened = open_store(&temp_dir).await;
    assert_eq!(
        reopened
            .get_node("project-1", &region_id)
            .await
            .expect("read should succeed"),
        Some(region)
    );
    assert_eq!(
        reopened
            .list_children("project-1", &project_id)
            .await
            .expect("children should load"),
        vec![root]
    );
    assert_eq!(
        reopened
            .list_children("project-1", &root_id)
            .await
            .expect("children should load"),
        vec![docs, src.clone()]
    );
    assert_eq!(
        reopened
            .list_children("project-1", &src_id)
            .await
            .expect("children should load"),
        vec![file]
    );
    assert_eq!(project.revision, 1);
    assert_eq!(src.value.relative_path.as_str(), "src");
}

#[tokio::test]
async fn storage_rejects_non_filesystem_parent_relationships() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let store = open_store(&temp_dir).await;
    let project_id = HierarchyNodeId::parse("node-project").expect("valid ID");
    store
        .create_node(project_id.clone(), project_node())
        .await
        .expect("project node should insert");

    let error = store
        .create_node(
            HierarchyNodeId::parse("node-file").expect("valid ID"),
            child_node(&project_id, NodeKind::File, "src/lib.rs"),
        )
        .await
        .expect_err("a file cannot be parented directly by the project");
    assert!(matches!(error, HierarchyStoreError::InvalidParent { .. }));
}

#[tokio::test]
async fn source_state_updates_are_revision_guarded() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let store = open_store(&temp_dir).await;
    let project_id = HierarchyNodeId::parse("node-project").expect("valid ID");
    let root_id = HierarchyNodeId::parse("node-root").expect("valid ID");
    store
        .create_node(project_id.clone(), project_node())
        .await
        .expect("project node should insert");
    let created = store
        .create_node(
            root_id.clone(),
            child_node(&project_id, NodeKind::Directory, ""),
        )
        .await
        .expect("root directory should insert");

    let replaced = store
        .update_source_state(
            "project-1",
            &root_id,
            HierarchySourceUpdate {
                expected_revision: created.revision,
                lifecycle: NodeLifecycle::Replaced,
                source_fingerprint: Some("sha256:def".to_string()),
            },
        )
        .await
        .expect("current revision should update");
    assert_eq!(
        replaced,
        HierarchyNode {
            lifecycle: NodeLifecycle::Replaced,
            revision: created.revision + 1,
            updated_at_ms: replaced.updated_at_ms,
            value: NewHierarchyNode {
                source_fingerprint: Some("sha256:def".to_string()),
                ..created.value.clone()
            },
            ..created.clone()
        }
    );

    let error = store
        .update_source_state(
            "project-1",
            &root_id,
            HierarchySourceUpdate {
                expected_revision: created.revision,
                lifecycle: NodeLifecycle::Missing,
                source_fingerprint: replaced.value.source_fingerprint.clone(),
            },
        )
        .await
        .expect_err("stale revision should be rejected");
    assert_eq!(
        error.to_string(),
        "hierarchy revision conflict: expected 1, found 2"
    );
}
