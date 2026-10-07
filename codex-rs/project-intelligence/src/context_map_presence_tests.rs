use std::fs;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::ContextMapStore;
use crate::HierarchyStore;
use crate::ProjectIndexRequest;
use crate::ProjectIndexer;

#[tokio::test]
async fn presence_distinguishes_a_never_indexed_project() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(root.path().join("README.md"), "# Recipes\n").expect("source fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let before = context_map
        .has_entries("project-1")
        .await
        .expect("presence loads");
    ProjectIndexer::new(hierarchy, context_map.clone())
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("refresh succeeds");

    assert_eq!(
        (
            before,
            context_map
                .has_entries("project-1")
                .await
                .expect("presence loads"),
            context_map
                .has_entries("project-2")
                .await
                .expect("presence loads"),
        ),
        (false, true, false)
    );
}
