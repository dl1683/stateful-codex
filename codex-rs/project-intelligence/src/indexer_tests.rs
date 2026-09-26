use std::fs;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ContextMapCoverage;
use crate::ContextMapQuery;
use crate::RegionAnchor;
use crate::SourceFingerprint;

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

    let mut grown = (1..=71)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>();
    grown[69] = "decisive_route_fact".to_string();
    fs::write(&source, grown.join("\n")).expect("source should grow");
    indexer
        .refresh_file(ProjectIndexFileRequest {
            project_id: "project-1".to_string(),
            project_root: root.path().to_path_buf(),
            relative_path: ProjectRelativePath::parse("facts.md").expect("relative path"),
        })
        .await
        .expect("grown source should refresh");
    let grown_regions = hierarchy
        .list_children("project-1", &file_id)
        .await
        .expect("grown regions should load");
    assert_eq!(
        grown_regions
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        indexed_regions
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        grown_regions[1].value.region_anchor,
        Some(RegionAnchor::new("lines", "65-71").expect("valid anchor"))
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
    assert_eq!(shrunk_regions.len(), 2,);
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
        1
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

#[tokio::test]
async fn failed_file_publication_preserves_the_complete_previous_generation() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(root.path().join("facts.md"), "old_generation_fact\n")
        .expect("source fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    indexer
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("initial generation should index");

    let root_text = root.path().display().to_string();
    let file_id =
        stable_id("file", &["project-1", &root_text, "facts.md"]).expect("stable file ID");
    let before = hierarchy
        .get_node("project-1", &file_id)
        .await
        .expect("file should load")
        .expect("file should exist");
    let conflicting_region = |description: &str| super::regions::ScannedRegion {
        start_line: 1,
        end_line: 1,
        description: description.to_string(),
        coverage: ContextMapCoverage::Complete,
    };
    let failed = super::publish::publish_file(
        &indexer,
        "project-1",
        &file_id,
        before.value.parent_id.clone().expect("file parent"),
        &super::scan::ScannedFile {
            project_root: root_text,
            relative_path: "facts.md".to_string(),
            fingerprint: SourceFingerprint::parse("sha256:new-generation")
                .expect("valid fingerprint"),
            description: "facts.md | new_generation_fact".to_string(),
            routing_terms: vec!["new_generation_fact".to_string()],
            coverage: ContextMapCoverage::Complete,
            regions: vec![
                conflicting_region("facts.md:1-1 | new_generation_fact"),
                conflicting_region("facts.md:1-1 | duplicate_anchor"),
            ],
        },
    )
    .await;
    assert!(failed.is_err());

    assert_eq!(
        hierarchy
            .get_node("project-1", &file_id)
            .await
            .expect("file should reload")
            .expect("file should remain"),
        before
    );
    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "old_generation_fact".to_string(),
                max_results: 10,
            })
            .await
            .expect("old generation query should succeed")
            .len(),
        1
    );
    assert!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "new_generation_fact".to_string(),
                max_results: 10,
            })
            .await
            .expect("new generation query should succeed")
            .is_empty()
    );
}
