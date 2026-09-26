use std::fs;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ContextMapQuery;
use crate::RegionAnchor;

#[tokio::test]
async fn refresh_builds_stable_regions_and_retires_removed_ranges() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let source = root.path().join("facts.md");
    let initial = (1..=70)
        .map(|line| {
            if line == 70 {
                "decisive_route_fact".to_string()
            } else {
                format!("line {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&source, initial).expect("source fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    let request = ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![root.path().to_path_buf()],
    };
    indexer
        .refresh(request.clone())
        .await
        .expect("project should index");

    let root_text = root.path().display().to_string();
    let file_id =
        stable_id("file", &["project-1", &root_text, "facts.md"]).expect("stable file ID");
    let indexed_regions = hierarchy
        .list_children("project-1", &file_id)
        .await
        .expect("regions should load");
    assert_eq!(indexed_regions.len(), 2);
    assert_eq!(
        indexed_regions
            .iter()
            .map(|node| node.value.region_anchor.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(RegionAnchor::new("lines", "1-64").expect("valid anchor")),
            Some(RegionAnchor::new("lines", "65-70").expect("valid anchor")),
        ]
    );
    let hits = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "decisive_route_fact".to_string(),
            max_results: 10,
        })
        .await
        .expect("region query should succeed");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].source.region_anchor,
        Some(RegionAnchor::new("lines", "65-70").expect("valid anchor"))
    );

    indexer
        .refresh(request.clone())
        .await
        .expect("unchanged refresh should succeed");
    assert_eq!(
        hierarchy
            .list_children("project-1", &file_id)
            .await
            .expect("regions should reload"),
        indexed_regions
    );

    fs::write(&source, "short\nsource\n").expect("source should shrink");
    indexer
        .refresh_file(ProjectIndexFileRequest {
            project_id: "project-1".to_string(),
            project_root: root.path().to_path_buf(),
            relative_path: ProjectRelativePath::parse("facts.md").expect("relative path"),
        })
        .await
        .expect("changed source should refresh");
    let shrunk_regions = hierarchy
        .list_children("project-1", &file_id)
        .await
        .expect("shrunk regions should load");
    assert_eq!(
        shrunk_regions
            .iter()
            .filter(|node| node.lifecycle == NodeLifecycle::Active)
            .map(|node| node.value.region_anchor.clone())
            .collect::<Vec<_>>(),
        vec![Some(
            RegionAnchor::new("lines", "1-2").expect("valid anchor")
        )]
    );
    assert_eq!(
        shrunk_regions
            .iter()
            .filter(|node| node.lifecycle == NodeLifecycle::Missing)
            .count(),
        2
    );

    fs::remove_file(source).expect("source should delete");
    let deletion = indexer
        .refresh_file(ProjectIndexFileRequest {
            project_id: "project-1".to_string(),
            project_root: root.path().to_path_buf(),
            relative_path: ProjectRelativePath::parse("facts.md").expect("relative path"),
        })
        .await
        .expect("deletion refresh should succeed");
    assert_eq!(
        deletion,
        ProjectIndexReport {
            files_indexed: 0,
            files_skipped: 0,
            missing_files: 1,
            truncated: false,
        }
    );
    assert!(
        hierarchy
            .list_children("project-1", &file_id)
            .await
            .expect("deleted regions should load")
            .iter()
            .all(|node| node.lifecycle == NodeLifecycle::Missing)
    );
}
