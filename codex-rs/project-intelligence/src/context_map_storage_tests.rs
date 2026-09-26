use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ContextMapSource;
use crate::HierarchySourceUpdate;
use crate::HierarchyStore;
use crate::NewHierarchyNode;
use crate::ProjectRelativePath;
use crate::RegionAnchor;

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

fn readme_source() -> ContextMapSource {
    ContextMapSource {
        project_root: "C:\\workspace".to_string(),
        relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
        region_anchor: None,
    }
}

#[test]
fn search_input_is_lowered_to_safe_literal_terms() {
    assert_eq!(
        search_expression("setup OR \"secret\"").expect("searchable terms"),
        "\"setup\" OR \"OR\" OR \"secret\""
    );
    assert_eq!(
        search_expression("deploy").expect("searchable term"),
        "\"deploy\"*"
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
    assert_eq!(
        context_map
            .create_entry(entry_id.clone(), created.value.clone())
            .await
            .expect("identical retry should return the existing entry"),
        created
    );
    let mut conflicting_entry = created.value.clone();
    conflicting_entry.description = "Different content for the same ID.".to_string();
    assert!(matches!(
        context_map
            .create_entry(entry_id.clone(), conflicting_entry)
            .await,
        Err(ContextMapStoreError::EntryIdentityConflict(id)) if id == entry_id.as_str()
    ));
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
            source: readme_source(),
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
            source: readme_source(),
            freshness: ContextMapFreshness::Stale,
        }]
    );
}

#[tokio::test]
async fn query_candidate_and_hit_materialization_share_one_read_snapshot() {
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
    let mut reader = context_map.pool.begin().await.expect("reader begins");
    let candidate_ids = sqlx::query_scalar::<_, String>(
        "SELECT entry.id
         FROM context_map_search AS search
         JOIN context_map_entries AS entry ON entry.rowid = search.rowid
         WHERE context_map_search MATCH ? AND entry.project_id = ?",
    )
    .bind("\"purpose\"*")
    .bind("project-1")
    .fetch_all(&mut *reader)
    .await
    .expect("candidate IDs load");
    assert_eq!(candidate_ids, vec![entry.id.to_string()]);

    hierarchy
        .update_source_state(
            "project-1",
            &file.id,
            HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:changed")),
            },
        )
        .await
        .expect("concurrent source update commits");

    let snapshot_hit = load_hit(&mut reader, "project-1", &entry.id)
        .await
        .expect("candidate materializes")
        .expect("candidate remains visible");
    assert_eq!(snapshot_hit.freshness, ContextMapFreshness::Current);
    reader.commit().await.expect("reader commits");

    let current = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "purpose".to_string(),
            max_results: 5,
        })
        .await
        .expect("current query succeeds");
    assert_eq!(current[0].freshness, ContextMapFreshness::Stale);
}

#[tokio::test]
async fn query_prefers_bounded_regions_without_one_source_crowding_results() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    let file = create_file(&hierarchy).await;
    let mut file_entry = new_entry("sha256:abc");
    file_entry.description = "shared route file summary".to_string();
    file_entry.routing_terms = vec!["shared".to_string(), "route".to_string()];
    context_map
        .create_entry(
            ContextMapEntryId::parse("map-readme").expect("valid entry ID"),
            file_entry,
        )
        .await
        .expect("file route should insert");

    for index in 0..4 {
        let node_id =
            HierarchyNodeId::parse(format!("node-region-{index}")).expect("valid region ID");
        let locator = format!("{}-{}", index * 10 + 1, index * 10 + 10);
        hierarchy
            .create_node(
                node_id.clone(),
                NewHierarchyNode {
                    project_id: "project-1".to_string(),
                    parent_id: Some(file.id.clone()),
                    kind: NodeKind::Region,
                    project_root: Some("C:\\workspace".to_string()),
                    relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
                    region_anchor: Some(RegionAnchor::new("lines", locator).expect("valid anchor")),
                    source_fingerprint: Some(fingerprint("sha256:abc")),
                },
            )
            .await
            .expect("region should insert");
        context_map
            .create_entry(
                ContextMapEntryId::parse(format!("map-region-{index}")).expect("valid entry ID"),
                NewContextMapEntry {
                    project_id: "project-1".to_string(),
                    node_id,
                    source_fingerprint: fingerprint("sha256:abc"),
                    description: format!("shared route region {index}"),
                    routing_terms: vec!["shared".to_string(), "route".to_string()],
                    coverage: ContextMapCoverage::Complete,
                },
            )
            .await
            .expect("region route should insert");
    }

    let other_file_id = HierarchyNodeId::parse("node-other-file").expect("valid file ID");
    hierarchy
        .create_node(
            other_file_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(HierarchyNodeId::parse("node-root").expect("valid root ID")),
                kind: NodeKind::File,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("OTHER.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:other")),
            },
        )
        .await
        .expect("other file should insert");
    context_map
        .create_entry(
            ContextMapEntryId::parse("map-other").expect("valid entry ID"),
            NewContextMapEntry {
                project_id: "project-1".to_string(),
                node_id: other_file_id,
                source_fingerprint: fingerprint("sha256:other"),
                description: "shared route other file".to_string(),
                routing_terms: vec!["shared".to_string(), "route".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("other route should insert");

    let hits = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "shared route".to_string(),
            max_results: 10,
        })
        .await
        .expect("query should succeed");
    assert_eq!(hits.len(), 4);
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.source.relative_path.as_str() == "README.md")
            .count(),
        3
    );
    assert!(
        hits.iter()
            .filter(|hit| hit.source.relative_path.as_str() == "README.md")
            .all(|hit| hit.source.region_anchor.is_some())
    );
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.source.relative_path.as_str() == "OTHER.md")
            .count(),
        1
    );

    hierarchy
        .update_source_state(
            "project-1",
            &file.id,
            HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:changed")),
            },
        )
        .await
        .expect("parent file should advance");
    let hits = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "shared route".to_string(),
            max_results: 10,
        })
        .await
        .expect("query should exclude prior-generation regions");
    assert!(
        hits.iter()
            .filter(|hit| hit.source.relative_path.as_str() == "README.md")
            .all(|hit| hit.source.region_anchor.is_none())
    );
}

#[tokio::test]
async fn query_preserves_results_beyond_one_busy_top_level_directory() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    create_file(&hierarchy).await;
    let root_id = HierarchyNodeId::parse("node-root").expect("valid root ID");
    for directory in ["reviews", "docs"] {
        hierarchy
            .create_node(
                HierarchyNodeId::parse(format!("node-{directory}")).expect("valid directory ID"),
                NewHierarchyNode {
                    project_id: "project-1".to_string(),
                    parent_id: Some(root_id.clone()),
                    kind: NodeKind::Directory,
                    project_root: Some("C:\\workspace".to_string()),
                    relative_path: ProjectRelativePath::parse(directory).expect("valid path"),
                    region_anchor: None,
                    source_fingerprint: Some(fingerprint("sha256:directory")),
                },
            )
            .await
            .expect("directory should insert");
    }
    for index in 0..5 {
        let node_id =
            HierarchyNodeId::parse(format!("node-review-{index}")).expect("valid file ID");
        let relative_path = format!("reviews/{index}.md");
        hierarchy
            .create_node(
                node_id.clone(),
                NewHierarchyNode {
                    project_id: "project-1".to_string(),
                    parent_id: Some(
                        HierarchyNodeId::parse("node-reviews").expect("valid directory ID"),
                    ),
                    kind: NodeKind::File,
                    project_root: Some("C:\\workspace".to_string()),
                    relative_path: ProjectRelativePath::parse(relative_path.clone())
                        .expect("valid path"),
                    region_anchor: None,
                    source_fingerprint: Some(fingerprint("sha256:review")),
                },
            )
            .await
            .expect("review file should insert");
        context_map
            .create_entry(
                ContextMapEntryId::parse(format!("map-a-review-{index}")).expect("valid entry ID"),
                NewContextMapEntry {
                    project_id: "project-1".to_string(),
                    node_id,
                    source_fingerprint: fingerprint("sha256:review"),
                    description: "shared route".to_string(),
                    routing_terms: Vec::new(),
                    coverage: ContextMapCoverage::Complete,
                },
            )
            .await
            .expect("review route should insert");
    }
    let docs_id = HierarchyNodeId::parse("node-docs-guide").expect("valid file ID");
    hierarchy
        .create_node(
            docs_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(HierarchyNodeId::parse("node-docs").expect("valid directory ID")),
                kind: NodeKind::File,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("docs/guide.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:docs")),
            },
        )
        .await
        .expect("docs file should insert");
    context_map
        .create_entry(
            ContextMapEntryId::parse("map-z-docs").expect("valid entry ID"),
            NewContextMapEntry {
                project_id: "project-1".to_string(),
                node_id: docs_id,
                source_fingerprint: fingerprint("sha256:docs"),
                description: "shared route".to_string(),
                routing_terms: Vec::new(),
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("docs route should insert");

    let hits = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "shared route".to_string(),
            max_results: 4,
        })
        .await
        .expect("query should succeed");
    assert_eq!(
        hits.iter()
            .map(|hit| hit.source.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "reviews/0.md",
            "reviews/1.md",
            "reviews/2.md",
            "docs/guide.md",
        ]
    );
}

#[tokio::test]
async fn query_pages_past_prior_generation_regions_to_find_a_current_route() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    let file = create_file(&hierarchy).await;
    for index in 0..17 {
        let node_id =
            HierarchyNodeId::parse(format!("node-stale-region-{index}")).expect("valid region ID");
        hierarchy
            .create_node(
                node_id.clone(),
                NewHierarchyNode {
                    project_id: "project-1".to_string(),
                    parent_id: Some(file.id.clone()),
                    kind: NodeKind::Region,
                    project_root: Some("C:\\workspace".to_string()),
                    relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
                    region_anchor: Some(
                        RegionAnchor::new("lines", format!("{}-{}", index + 1, index + 1))
                            .expect("valid anchor"),
                    ),
                    source_fingerprint: Some(fingerprint("sha256:abc")),
                },
            )
            .await
            .expect("region should insert");
        context_map
            .create_entry(
                ContextMapEntryId::parse(format!("map-stale-region-{index}"))
                    .expect("valid entry ID"),
                NewContextMapEntry {
                    project_id: "project-1".to_string(),
                    node_id,
                    source_fingerprint: fingerprint("sha256:abc"),
                    description: "needle".to_string(),
                    routing_terms: Vec::new(),
                    coverage: ContextMapCoverage::Complete,
                },
            )
            .await
            .expect("region route should insert");
    }
    hierarchy
        .update_source_state(
            "project-1",
            &file.id,
            HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:changed")),
            },
        )
        .await
        .expect("parent file should advance");
    let current_file_id = HierarchyNodeId::parse("node-current-file").expect("valid file ID");
    hierarchy
        .create_node(
            current_file_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(HierarchyNodeId::parse("node-root").expect("valid root ID")),
                kind: NodeKind::File,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("CURRENT.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:current")),
            },
        )
        .await
        .expect("current file should insert");
    let current = context_map
        .create_entry(
            ContextMapEntryId::parse("map-current").expect("valid entry ID"),
            NewContextMapEntry {
                project_id: "project-1".to_string(),
                node_id: current_file_id,
                source_fingerprint: fingerprint("sha256:current"),
                description: "needle".to_string(),
                routing_terms: Vec::new(),
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("current route should insert");

    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "needle".to_string(),
                max_results: 1,
            })
            .await
            .expect("query should find the current route"),
        vec![ContextMapHit {
            entry: current,
            source: ContextMapSource {
                project_root: "C:\\workspace".to_string(),
                relative_path: ProjectRelativePath::parse("CURRENT.md").expect("valid path"),
                region_anchor: None,
            },
            freshness: ContextMapFreshness::Current,
        }]
    );
}

#[tokio::test]
async fn guarded_reindex_replaces_the_search_document_for_the_current_source() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    let file = create_file(&hierarchy).await;
    let entry_id = ContextMapEntryId::parse("map-readme").expect("valid entry ID");
    let created = context_map
        .create_entry(entry_id.clone(), new_entry("sha256:abc"))
        .await
        .expect("context-map entry should insert");
    let current_file = hierarchy
        .update_source_state(
            "project-1",
            &file.id,
            HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(fingerprint("sha256:def")),
            },
        )
        .await
        .expect("source fingerprint should update");
    let updated = context_map
        .update_entry(
            "project-1",
            &entry_id,
            ContextMapEntryUpdate {
                expected_revision: created.revision,
                source_fingerprint: fingerprint("sha256:def"),
                description: "Installation and deployment entry points.".to_string(),
                routing_terms: vec!["deploy".to_string(), "install".to_string()],
                coverage: ContextMapCoverage::Partial,
            },
        )
        .await
        .expect("current revision should update");
    assert_eq!(updated.revision, created.revision + 1);
    assert_eq!(
        updated.freshness_against(&current_file),
        Ok(ContextMapFreshness::Current)
    );
    assert!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "purpose".to_string(),
                max_results: 5,
            })
            .await
            .expect("old search should succeed")
            .is_empty()
    );
    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "deployment".to_string(),
                max_results: 5,
            })
            .await
            .expect("new search should succeed"),
        vec![ContextMapHit {
            entry: updated,
            source: readme_source(),
            freshness: ContextMapFreshness::Current,
        }]
    );

    let error = context_map
        .update_entry(
            "project-1",
            &entry_id,
            ContextMapEntryUpdate {
                expected_revision: created.revision,
                source_fingerprint: fingerprint("sha256:def"),
                description: "Stale writer".to_string(),
                routing_terms: Vec::new(),
                coverage: ContextMapCoverage::Partial,
            },
        )
        .await
        .expect_err("stale revision should be rejected");
    assert_eq!(
        error.to_string(),
        "context-map revision conflict: expected 1, found 2"
    );
}
