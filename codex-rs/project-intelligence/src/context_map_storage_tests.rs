use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ContextMapSource;
use crate::EvidenceLineRange;
use crate::EvidenceRoute;
use crate::HierarchySourceUpdate;
use crate::HierarchyStore;
use crate::IndexedExtraction;
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
        indexed_extraction: None,
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
async fn office_region_attestation_round_trips_and_serializes_separately_from_anchor() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    let file = create_file(&hierarchy).await;
    let region_id = HierarchyNodeId::parse("node-docx-region").expect("valid region ID");
    hierarchy
        .create_node(
            region_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(file.id),
                kind: NodeKind::Region,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
                region_anchor: Some(
                    RegionAnchor::new("docx-paragraph", "body/p[1]").expect("valid anchor"),
                ),
                source_fingerprint: Some(fingerprint("sha256:abc")),
            },
        )
        .await
        .expect("region should insert");
    let extraction = IndexedExtraction::new("codex-docx", "2", "sha256:representation")
        .expect("valid extraction");
    sqlx::query(
        "INSERT INTO region_extraction_attestations (
             region_node_id, extractor_name, extractor_version,
             canonical_representation_digest
         ) VALUES (?, ?, ?, ?)",
    )
    .bind(region_id.as_str())
    .bind(&extraction.extractor_name)
    .bind(&extraction.extractor_version)
    .bind(&extraction.canonical_representation_digest)
    .execute(&context_map.pool)
    .await
    .expect("attestation should insert");
    let entry = context_map
        .create_entry(
            ContextMapEntryId::parse("map-docx-region").expect("valid entry ID"),
            NewContextMapEntry {
                project_id: "project-1".to_string(),
                node_id: region_id,
                source_fingerprint: fingerprint("sha256:abc"),
                description: "distinctive office clause".to_string(),
                routing_terms: Vec::new(),
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("region entry should insert");
    let hit = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "distinctive office clause".to_string(),
            max_results: 1,
        })
        .await
        .expect("region query should succeed")
        .data
        .pop()
        .expect("region hit should exist");
    assert_eq!(hit.entry, entry);
    assert_eq!(hit.source.indexed_extraction, Some(extraction.clone()));
    let route = EvidenceRoute::from_hit(&hit).expect("route should build");
    assert_eq!(route.region_anchor, hit.source.region_anchor);
    assert_eq!(route.indexed_extraction, Some(extraction));
    assert_eq!(route.line_range, None);
}

#[tokio::test]
async fn migration_0015_upgrades_a_populated_0014_database() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    create_file(&hierarchy).await;
    let entry_id = ContextMapEntryId::parse("map-populated").expect("valid entry ID");
    let entry = context_map
        .create_entry(entry_id.clone(), new_entry("sha256:abc"))
        .await
        .expect("populated entry should insert");
    sqlx::query("DROP TABLE region_extraction_attestations")
        .execute(&context_map.pool)
        .await
        .expect("0015 table should drop for upgrade simulation");
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 15")
        .execute(&context_map.pool)
        .await
        .expect("0015 migration marker should delete");
    drop(context_map);
    drop(hierarchy);

    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let reopened = ContextMapStore::open(&sqlite)
        .await
        .expect("0014 database should upgrade");
    let reopened_entry = reopened
        .get_entry("project-1", &entry_id)
        .await
        .expect("populated entry should survive upgrade");
    assert_eq!(reopened_entry, Some(entry));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'region_extraction_attestations'",
        )
        .fetch_one(&reopened.pool)
        .await
        .expect("upgraded table should exist"),
        1
    );
}

#[tokio::test]
async fn corrupt_partial_and_all_null_attestation_rows_are_rejected() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    let file = create_file(&hierarchy).await;
    let region_id = HierarchyNodeId::parse("node-corrupt-region").expect("valid region ID");
    hierarchy
        .create_node(
            region_id.clone(),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(file.id),
                kind: NodeKind::Region,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("README.md").expect("valid path"),
                region_anchor: Some(
                    RegionAnchor::new("docx-paragraph", "body/p[1]").expect("valid anchor"),
                ),
                source_fingerprint: Some(fingerprint("sha256:abc")),
            },
        )
        .await
        .expect("region should insert");
    let entry_id = ContextMapEntryId::parse("map-corrupt-region").expect("valid entry ID");
    context_map
        .create_entry(
            entry_id.clone(),
            NewContextMapEntry {
                project_id: "project-1".to_string(),
                node_id: region_id.clone(),
                source_fingerprint: fingerprint("sha256:abc"),
                description: "corrupt attestation fixture".to_string(),
                routing_terms: Vec::new(),
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("region entry should insert");
    sqlx::query(
        "INSERT INTO region_extraction_attestations (
             region_node_id, extractor_name, extractor_version,
             canonical_representation_digest
         ) VALUES (?, ?, ?, ?)",
    )
    .bind(region_id.as_str())
    .bind("codex-docx")
    .bind("")
    .bind("sha256:digest")
    .execute(&context_map.pool)
    .await
    .expect("partial invalid row should persist for read validation");
    assert!(matches!(
        context_map.get_hit("project-1", &entry_id).await,
        Err(ContextMapStoreError::CorruptEntry(id)) if id == region_id.as_str()
    ));
    let all_null = sqlx::query(
        "INSERT INTO region_extraction_attestations (
             region_node_id, extractor_name, extractor_version,
             canonical_representation_digest
         ) VALUES (?, NULL, NULL, NULL)",
    )
    .bind("node-all-null")
    .execute(&context_map.pool)
    .await;
    assert!(all_null.is_err());
}

#[test]
fn evidence_routes_serialize_the_three_locator_shapes_exactly() {
    let context_map_entry_id = ContextMapEntryId::parse("map-route").expect("valid entry ID");
    let source_fingerprint = fingerprint("sha256:source");
    let extraction = IndexedExtraction::new("codex-docx", "2", "sha256:representation")
        .expect("valid extraction");
    let file_route = EvidenceRoute {
        context_map_entry_id: context_map_entry_id.clone(),
        source_fingerprint: source_fingerprint.clone(),
        line_range: None,
        region_anchor: None,
        indexed_extraction: None,
    };
    let text_route = EvidenceRoute {
        context_map_entry_id: context_map_entry_id.clone(),
        source_fingerprint: source_fingerprint.clone(),
        line_range: Some(EvidenceLineRange { start: 4, end: 6 }),
        region_anchor: None,
        indexed_extraction: None,
    };
    let docx_route = EvidenceRoute {
        context_map_entry_id,
        source_fingerprint,
        line_range: None,
        region_anchor: Some(
            RegionAnchor::new("docx-paragraph", "body/p[2]").expect("valid DOCX anchor"),
        ),
        indexed_extraction: Some(extraction),
    };

    assert_eq!(
        serde_json::to_value(file_route).expect("file route should serialize"),
        serde_json::json!({
            "contextMapEntryId": "map-route",
            "sourceFingerprint": "sha256:source",
            "lineRange": null,
            "regionAnchor": null,
            "indexedExtraction": null,
        })
    );
    assert_eq!(
        serde_json::to_value(text_route).expect("text route should serialize"),
        serde_json::json!({
            "contextMapEntryId": "map-route",
            "sourceFingerprint": "sha256:source",
            "lineRange": {"start": 4, "end": 6},
            "regionAnchor": null,
            "indexedExtraction": null,
        })
    );
    assert_eq!(
        serde_json::to_value(docx_route).expect("DOCX route should serialize"),
        serde_json::json!({
            "contextMapEntryId": "map-route",
            "sourceFingerprint": "sha256:source",
            "lineRange": null,
            "regionAnchor": {"scheme": "docx-paragraph", "locator": "body/p[2]"},
            "indexedExtraction": {
                "extractorName": "codex-docx",
                "extractorVersion": "2",
                "canonicalRepresentationDigest": "sha256:representation",
            },
        })
    );
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
            .expect("query should succeed")
            .data,
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
            .expect("stale map should remain discoverable")
            .data,
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
    assert_eq!(current.data[0].freshness, ContextMapFreshness::Stale);
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
        .expect("query should succeed")
        .data;
    // One source cannot crowd out others: README's capped fourth region is ordered
    // after every other source and only backfills because the result still has room.
    assert_eq!(hits.len(), 5);
    assert_eq!(
        hits.iter()
            .map(|hit| hit.source.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "OTHER.md",
            "README.md",
            "README.md",
            "README.md",
            "README.md"
        ]
    );
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.source.relative_path.as_str() == "README.md")
            .count(),
        4
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
        .expect("query should exclude prior-generation regions")
        .data;
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
    for index in 0..160 {
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

    let result = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "shared route".to_string(),
            max_results: 10,
        })
        .await
        .expect("query should succeed");
    assert_eq!(
        result
            .data
            .iter()
            .map(|hit| hit.source.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "reviews/0.md",
            "reviews/1.md",
            "reviews/10.md",
            "docs/guide.md",
            "reviews/100.md",
            "reviews/101.md",
            "reviews/102.md",
            "reviews/103.md",
            "reviews/104.md",
            "reviews/105.md",
        ]
    );
    assert!(result.truncated);

    let limited = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "shared route".to_string(),
            max_results: 3,
        })
        .await
        .expect("limited query should succeed");
    assert_eq!(
        limited
            .data
            .iter()
            .map(|hit| hit.source.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["reviews/0.md", "reviews/1.md", "reviews/10.md"]
    );
    assert!(limited.truncated);
}

#[tokio::test]
async fn query_returns_every_match_in_one_directory_when_room_remains() {
    let temp_dir = TempDir::new().expect("tempdir should be created");
    let (hierarchy, context_map) = stores(&temp_dir).await;
    create_file(&hierarchy).await;
    hierarchy
        .create_node(
            HierarchyNodeId::parse("node-src").expect("valid directory ID"),
            NewHierarchyNode {
                project_id: "project-1".to_string(),
                parent_id: Some(HierarchyNodeId::parse("node-root").expect("valid root ID")),
                kind: NodeKind::Directory,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("src").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint("sha256:directory")),
            },
        )
        .await
        .expect("directory should insert");
    for index in 0..5 {
        let node_id = HierarchyNodeId::parse(format!("node-src-{index}")).expect("valid file ID");
        hierarchy
            .create_node(
                node_id.clone(),
                NewHierarchyNode {
                    project_id: "project-1".to_string(),
                    parent_id: Some(HierarchyNodeId::parse("node-src").expect("valid ID")),
                    kind: NodeKind::File,
                    project_root: Some("C:\\workspace".to_string()),
                    relative_path: ProjectRelativePath::parse(format!("src/{index}.rs"))
                        .expect("valid path"),
                    region_anchor: None,
                    source_fingerprint: Some(fingerprint("sha256:src")),
                },
            )
            .await
            .expect("source file should insert");
        context_map
            .create_entry(
                ContextMapEntryId::parse(format!("map-src-{index}")).expect("valid entry ID"),
                NewContextMapEntry {
                    project_id: "project-1".to_string(),
                    node_id,
                    source_fingerprint: fingerprint("sha256:src"),
                    description: "decisive invariant".to_string(),
                    routing_terms: Vec::new(),
                    coverage: ContextMapCoverage::Complete,
                },
            )
            .await
            .expect("source route should insert");
    }

    let result = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "decisive invariant".to_string(),
            max_results: 10,
        })
        .await
        .expect("query should succeed");

    assert_eq!(
        result
            .data
            .iter()
            .map(|hit| hit.source.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["src/0.rs", "src/1.rs", "src/2.rs", "src/3.rs", "src/4.rs"]
    );
    assert!(!result.truncated);
}

#[tokio::test]
async fn query_scans_past_prior_generation_regions_to_find_a_current_route() {
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
            .expect("query should find the current route")
            .data,
        vec![ContextMapHit {
            entry: current,
            source: ContextMapSource {
                project_root: "C:\\workspace".to_string(),
                relative_path: ProjectRelativePath::parse("CURRENT.md").expect("valid path"),
                region_anchor: None,
                indexed_extraction: None,
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
            .data
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
            .expect("new search should succeed")
            .data,
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
