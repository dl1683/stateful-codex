use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::HierarchySourceUpdate;
use crate::HierarchyStore;
use crate::NewHierarchyNode;
use crate::ProjectRelativePath;

fn fingerprint(value: &str) -> SourceFingerprint {
    SourceFingerprint::parse(value).expect("valid fingerprint")
}

async fn stores(temp_dir: &TempDir) -> (HierarchyStore, ContextMapStore) {
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy store should open");
    let context_map = ContextMapStore::open(&sqlite)
        .await
        .expect("context-map store should open");
    (hierarchy, context_map)
}

async fn create_file(hierarchy: &HierarchyStore) -> HierarchyNode {
    let project_id = HierarchyNodeId::parse("node-project").expect("valid node ID");
    let root_id = HierarchyNodeId::parse("node-root").expect("valid node ID");
    hierarchy
        .create_node(
            project_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .expect("project node should insert");
    hierarchy
        .create_node(
            root_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(project_id),
                kind: NodeKind::Directory,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:root")),
            },
        )
        .await
        .expect("root node should insert");
    hierarchy
        .create_node(
            HierarchyNodeId::parse("node-file").expect("valid node ID"),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(root_id),
                kind: NodeKind::File,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:abc")),
            },
        )
        .await
        .expect("file node should insert")
}

fn new_entry(source_fingerprint: &str) -> NewContextMapEntry {
    NewContextMapEntry {
        project_id: "project-1".to_string(),
        node_id: HierarchyNodeId::parse("node-file").expect("valid node ID"),
        source_fingerprint: fingerprint(source_fingerprint),
        description: "Project purpose, setup, and operator instructions.".to_string(),
        routing_terms: vec!["purpose".to_string(), "setup".to_string()],
        coverage: ContextMapCoverage::Complete,
    }
}

#[test]
fn search_input_is_lowered_to_literal_prefix_terms() {
    assert_eq!(
        search_expression("setup OR \"secret\"").expect("searchable terms"),
        "\"setup\"* OR \"OR\"* OR \"secret\"*"
    );
    assert!(matches!(
        search_expression("!!!"),
        Err(ContextMapStoreError::InvalidEntry(
            ContextMapError::NoSearchTerms
        ))
    ));
}

#[tokio::test]
async fn context_map_entry_survives_reopen_with_routing_term_order() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    create_file(&hierarchy).await;
    let entry_id = ContextMapEntryId::parse("map-readme").expect("valid entry ID");
    let created = context_map
        .create_entry(entry_id.clone(), new_entry("sha256:abc"))
        .await
        .expect("context-map entry should insert");
    drop(context_map);
    drop(hierarchy);

    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let reopened = ContextMapStore::open(&sqlite)
        .await
        .expect("context-map store should reopen");
    assert_eq!(
        reopened
            .get_entry("project-1", &entry_id)
            .await
            .expect("entry should load"),
        Some(created)
    );
}

#[tokio::test]
async fn context_map_rejects_a_stale_source_fingerprint() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    create_file(&hierarchy).await;

    let error = context_map
        .create_entry(
            ContextMapEntryId::parse("map-readme").expect("valid entry ID"),
            new_entry("sha256:stale"),
        )
        .await
        .expect_err("stale source should be rejected");
    assert!(matches!(
        error,
        ContextMapStoreError::SourceNotCurrent(ContextMapFreshness::Stale)
    ));
}

#[tokio::test]
async fn bounded_query_returns_current_and_then_stale_routing_metadata() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    let file = create_file(&hierarchy).await;
    let entry = context_map
        .create_entry(
            ContextMapEntryId::parse("map-readme").expect("valid entry ID"),
            new_entry("sha256:abc"),
        )
        .await
        .expect("context-map entry should insert");
    let query = ContextMapQuery {
        project_id: "project-1".to_string(),
        text: "operator setup".to_string(),
        max_results: 5,
    };
    assert_eq!(
        context_map
            .query(query.clone())
            .await
            .expect("query should succeed"),
        vec![ContextMapHit {
            entry: entry.clone(),
            freshness: ContextMapFreshness::Current,
        }]
    );

    hierarchy
        .update_source_state(
            "project-1",
            &file.id,
            HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: NodeLifecycle::Replaced,
                source_fingerprint: Some(fingerprint("sha256:def")),
            },
        )
        .await
        .expect("source state should update");
    assert_eq!(
        context_map
            .query(query)
            .await
            .expect("stale map should remain discoverable"),
        vec![ContextMapHit {
            entry,
            freshness: ContextMapFreshness::Stale,
        }]
    );
}
