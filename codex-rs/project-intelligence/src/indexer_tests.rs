use std::fs;
use std::io::Cursor;
use std::io::Write;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::ContextMapCoverage;
use crate::ContextMapFreshness;
use crate::ContextMapListQuery;
use crate::ContextMapQuery;
use crate::HierarchyRegionSourceUpdate;
use crate::IndexedExtraction;
use crate::NodeLifecycle;
use crate::RegionAnchor;
use crate::SourceFingerprint;

fn generated_docx(body: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
    writer
        .start_file("word/document.xml", SimpleFileOptions::default())
        .expect("document part should start");
    writer
        .write_all(
            format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
            )
            .as_bytes(),
        )
        .expect("document part should write");
    writer.finish().expect("package should finish");
    bytes
}

#[tokio::test]
async fn docx_indexing_publishes_structural_regions_and_v2_attestation() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(
        root.path().join("purchase-agreement.docx"),
        generated_docx(
            r#"<w:p><w:r><w:t>body clause</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>distinctive table clause</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
        ),
    )
    .expect("DOCX fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    let report = indexer
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("DOCX project should index");
    assert_eq!(report.files_indexed, 1);
    assert_eq!(report.regions_indexed, 2);
    assert_eq!(report.files_skipped, 0);
    assert!(report.region_coverage_complete);

    let root_text = root.path().display().to_string();
    let file_id = stable_id(
        "file",
        &["project-1", &root_text, "purchase-agreement.docx"],
    )
    .expect("stable file ID");
    let regions = hierarchy
        .list_children("project-1", &file_id)
        .await
        .expect("DOCX regions should load");
    assert_eq!(
        regions
            .iter()
            .map(|region| region.value.region_anchor.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(RegionAnchor::new("docx-paragraph", "body/p[1]").expect("body anchor")),
            Some(
                RegionAnchor::new("docx-paragraph", "body/tbl[1]/tr[1]/tc[1]/p[1]")
                    .expect("table anchor")
            ),
        ]
    );
    let hit = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "distinctive table clause".to_string(),
            max_results: 1,
        })
        .await
        .expect("table clause should be searchable")
        .data
        .pop()
        .expect("table region should be returned");
    assert_eq!(
        hit.source.region_anchor,
        Some(RegionAnchor::new("docx-paragraph", "body/tbl[1]/tr[1]/tc[1]/p[1]").expect("anchor"))
    );
    assert_eq!(
        hit.source.indexed_extraction,
        Some(
            IndexedExtraction::new(
                "codex-docx",
                "2",
                "sha256:f50fc47da7e122f07029464c66aa97ba16d8bcf9a7609e978d25f550f76efcfb",
            )
            .expect("valid extraction"),
        )
    );
}

#[tokio::test]
async fn over_limit_docx_is_published_as_one_partial_file_without_skips_or_regions() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let fixture = generated_docx(r#"<w:p><w:r><w:t>bounded content</w:t></w:r></w:p>"#);
    fs::write(root.path().join("bounded.docx"), &fixture).expect("DOCX fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    let mut report = indexer
        .refresh_with_office_limit(
            ProjectIndexRequest {
                project_id: "project-1".to_string(),
                roots: vec![root.path().to_path_buf()],
            },
            16,
        )
        .await
        .expect("over-limit DOCX should publish");
    report.scan_duration_ms = 0;
    report.publication_duration_ms = 0;
    assert_eq!(
        report,
        ProjectIndexReport {
            inventory_complete: true,
            region_coverage_complete: false,
            files_indexed: 1,
            regions_indexed: 0,
            files_skipped: 0,
            missing_files: 0,
            truncated: false,
            scan_duration_ms: 0,
            publication_duration_ms: 0,
        }
    );
    let files = context_map
        .list_project(ContextMapListQuery {
            project_id: "project-1".to_string(),
            max_results: 10,
        })
        .await
        .expect("partial file should be queryable");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].entry.value.coverage, ContextMapCoverage::Partial);
    assert_eq!(
        files[0].entry.value.source_fingerprint.as_str(),
        format!("sha256:{:x}", Sha256::digest(&fixture))
    );
    assert!(
        hierarchy
            .list_children("project-1", &files[0].entry.value.node_id)
            .await
            .expect("file children should load")
            .is_empty()
    );
}

#[tokio::test]
async fn published_docx_failure_uses_stable_code_without_an_adversarial_zip_name() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let adversarial_name = format!("word/{}-secret.xml", "x".repeat(2_000));
    let mut bytes = Vec::new();
    let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
    writer
        .start_file(&adversarial_name, SimpleFileOptions::default())
        .expect("adversarial ZIP entry should start");
    writer
        .write_all(b"not a document part")
        .expect("adversarial ZIP entry should write");
    writer.finish().expect("adversarial ZIP should finish");
    fs::write(root.path().join("fixture.docx"), bytes).expect("DOCX fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let report = ProjectIndexer::new(hierarchy, context_map.clone())
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("failure should publish");
    assert_eq!(report.files_skipped, 0);
    let expected = "fixture.docx | office extraction failed: corrupt";
    let file = context_map
        .list_project(ContextMapListQuery {
            project_id: "project-1".to_string(),
            max_results: 10,
        })
        .await
        .expect("published file should load")
        .pop()
        .expect("published file should exist");
    assert_eq!(file.entry.value.description, expected);
    assert!(!file.entry.value.description.contains(&adversarial_name));
    let hit = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "fixture".to_string(),
            max_results: 1,
        })
        .await
        .expect("published failure should be searchable")
        .data
        .pop()
        .expect("published failure hit should exist");
    assert_eq!(hit.entry.value.description, expected);
    assert!(!hit.entry.value.description.contains(&adversarial_name));
}

#[tokio::test]
async fn region_attestation_replacement_is_atomic_and_rollback_preserves_all_fields() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(
        root.path().join("agreement.docx"),
        generated_docx(r#"<w:p><w:r><w:t>initial clause</w:t></w:r></w:p>"#),
    )
    .expect("initial DOCX should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    indexer
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("initial DOCX should index");
    let root_text = root.path().display().to_string();
    let file_id =
        stable_id("file", &["project-1", &root_text, "agreement.docx"]).expect("stable file ID");
    let file = hierarchy
        .get_node("project-1", &file_id)
        .await
        .expect("file should load")
        .expect("file should exist");
    let replacement = IndexedExtraction::new("replacement-extractor", "9", "sha256:replacement")
        .expect("valid replacement extraction");
    super::publish::publish_file(
        &indexer,
        "project-1",
        &file_id,
        file.value.parent_id.clone().expect("file parent"),
        &super::scan::ScannedFile {
            project_root: root_text.clone(),
            relative_path: "agreement.docx".to_string(),
            fingerprint: SourceFingerprint::parse("sha256:replacement-source")
                .expect("valid fingerprint"),
            description: "agreement.docx | replacement clause".to_string(),
            routing_terms: Vec::new(),
            coverage: ContextMapCoverage::Complete,
            regions: vec![super::regions::ScannedRegion {
                anchor: RegionAnchor::new("docx-paragraph", "body/p[1]").expect("valid anchor"),
                indexed_extraction: Some(replacement.clone()),
                description: "agreement.docx | replacement clause".to_string(),
                coverage: ContextMapCoverage::Complete,
            }],
        },
        super::PublicationFence::Targeted,
    )
    .await
    .expect("replacement should commit");
    let replacement_hit = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "replacement clause".to_string(),
            max_results: 1,
        })
        .await
        .expect("replacement should query")
        .data
        .pop()
        .expect("replacement hit should exist");
    assert_eq!(
        replacement_hit.source.indexed_extraction,
        Some(replacement.clone())
    );

    let rollback = IndexedExtraction::new("rollback-extractor", "10", "sha256:rollback")
        .expect("valid rollback extraction");
    let failed = super::publish::publish_file(
        &indexer,
        "project-1",
        &file_id,
        file.value.parent_id.expect("file parent"),
        &super::scan::ScannedFile {
            project_root: root_text,
            relative_path: "agreement.docx".to_string(),
            fingerprint: SourceFingerprint::parse("sha256:rollback-source")
                .expect("valid fingerprint"),
            description: "agreement.docx | rollback clause".to_string(),
            routing_terms: Vec::new(),
            coverage: ContextMapCoverage::Complete,
            regions: vec![
                super::regions::ScannedRegion {
                    anchor: RegionAnchor::new("docx-paragraph", "body/p[1]").expect("valid anchor"),
                    indexed_extraction: Some(rollback.clone()),
                    description: "agreement.docx | rollback clause".to_string(),
                    coverage: ContextMapCoverage::Complete,
                },
                super::regions::ScannedRegion {
                    anchor: RegionAnchor::new("docx-paragraph", "body/p[1]").expect("valid anchor"),
                    indexed_extraction: Some(rollback),
                    description: "agreement.docx | duplicate rollback clause".to_string(),
                    coverage: ContextMapCoverage::Complete,
                },
            ],
        },
        super::PublicationFence::Targeted,
    )
    .await;
    assert!(failed.is_err());
    let after_rollback = context_map
        .get_hit("project-1", &replacement_hit.entry.id)
        .await
        .expect("replacement hit should reload")
        .expect("replacement hit should remain");
    assert_eq!(after_rollback.source.indexed_extraction, Some(replacement));
}

#[tokio::test]
async fn retired_docx_regions_fail_the_guarded_hit_check() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(
        root.path().join("agreement.docx"),
        generated_docx(r#"<w:p><w:r><w:t>retired clause</w:t></w:r></w:p>"#),
    )
    .expect("DOCX fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    ProjectIndexer::new(hierarchy.clone(), context_map.clone())
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("DOCX should index");
    let hit = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "retired clause".to_string(),
            max_results: 1,
        })
        .await
        .expect("region should query")
        .data
        .pop()
        .expect("region hit should exist");
    let region = hierarchy
        .get_node("project-1", &hit.entry.value.node_id)
        .await
        .expect("region should load")
        .expect("region should exist");
    hierarchy
        .update_region_source(
            "project-1",
            &region.id,
            HierarchyRegionSourceUpdate {
                expected_revision: region.revision,
                lifecycle: NodeLifecycle::Missing,
                region_anchor: region.value.region_anchor.expect("region anchor"),
                source_fingerprint: region.value.source_fingerprint.expect("region fingerprint"),
            },
        )
        .await
        .expect("region should retire");
    assert!(matches!(
        context_map
            .get_guarded_hit(
                "project-1",
                &hit.entry.id,
                &hit.entry.value.source_fingerprint
            )
            .await,
        Err(crate::ContextMapStoreError::SourceNotCurrent(
            ContextMapFreshness::Stale
        ))
    ));
}

#[tokio::test]
async fn failed_docx_is_published_as_a_partial_file_without_a_skip() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(
        root.path().join("broken.docx"),
        b"not a zip and never an archive entry name",
    )
    .expect("corrupt DOCX fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    let report = indexer
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("failed DOCX should still index");
    assert_eq!(report.files_indexed, 1);
    assert_eq!(report.regions_indexed, 0);
    assert_eq!(report.files_skipped, 0);
    assert!(!report.region_coverage_complete);
    let files = context_map
        .list_project(ContextMapListQuery {
            project_id: "project-1".to_string(),
            max_results: 10,
        })
        .await
        .expect("partial file should be queryable");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].entry.value.coverage, ContextMapCoverage::Partial);
    assert!(files[0].entry.value.description.contains("corrupt"));
    assert_eq!(
        hierarchy
            .list_children("project-1", &files[0].entry.value.node_id)
            .await
            .expect("file children should load")
            .len(),
        0
    );
}

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
    let refresh = hierarchy
        .project_intelligence_status("project-1")
        .await
        .expect("project status should load")
        .last_refresh
        .expect("full refresh status should persist");
    assert!(refresh.inventory_complete);
    assert!(refresh.region_coverage_complete);
    assert_eq!(refresh.files_indexed, 1);
    assert_eq!(refresh.regions_indexed, 2);
    assert_eq!(refresh.files_skipped, 0);

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
        .expect("region query should succeed")
        .data;
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].source.region_anchor,
        Some(RegionAnchor::new("lines", "65-70").expect("valid anchor"))
    );
    assert_eq!(hits[0].source.indexed_extraction, None);

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
    let mut stable_deletion = deletion;
    stable_deletion.scan_duration_ms = 0;
    stable_deletion.publication_duration_ms = 0;
    assert_eq!(
        stable_deletion,
        ProjectIndexReport {
            inventory_complete: true,
            region_coverage_complete: true,
            files_indexed: 0,
            regions_indexed: 0,
            files_skipped: 0,
            missing_files: 1,
            truncated: false,
            scan_duration_ms: 0,
            publication_duration_ms: 0,
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
        anchor: RegionAnchor::new("lines", "1-1").expect("valid anchor"),
        indexed_extraction: None,
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
        PublicationFence::Targeted,
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
            .data
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
            .data
            .is_empty()
    );
}

#[tokio::test]
async fn transient_read_failure_does_not_reconcile_the_unread_file_as_missing() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    fs::write(root.path().join("facts.md"), "last_complete_generation\n")
        .expect("source fixture should write");
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
        .expect("initial generation should index");
    let root_text = root.path().display().to_string();
    let file_id =
        stable_id("file", &["project-1", &root_text, "facts.md"]).expect("stable file ID");
    let before = hierarchy
        .get_node("project-1", &file_id)
        .await
        .expect("file should load")
        .expect("file should exist");

    let failed_scan = super::scan::scan_roots_with_limits(
        &request.roots,
        super::scan::ScanLimits {
            max_files: 10,
            max_project_regions: 10,
        },
        |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected transient read failure",
            )
            .into())
        },
    )
    .expect("a file read failure should produce an incomplete scan");
    assert!(!failed_scan.inventory_complete);
    assert_eq!(failed_scan.files_skipped, 1);
    let failed_generation = super::generation::claim(&indexer, "project-1")
        .await
        .expect("failed refresh should claim a generation");

    let report = indexer
        .publish_refresh(request.clone(), failed_scan, failed_generation)
        .await
        .expect("incomplete inventory should publish without deletion reconciliation");
    assert_eq!(
        report,
        ProjectIndexReport {
            inventory_complete: false,
            region_coverage_complete: true,
            files_indexed: 0,
            regions_indexed: 0,
            files_skipped: 1,
            missing_files: 0,
            truncated: true,
            scan_duration_ms: 0,
            publication_duration_ms: 0,
        }
    );
    indexer
        .record_refresh_status("project-1", &report, failed_generation)
        .await
        .expect("incomplete refresh health should persist");
    let refresh = HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy should reopen")
        .project_intelligence_status("project-1")
        .await
        .expect("project status should survive reopen")
        .last_refresh
        .expect("incomplete refresh status should survive reopen");
    assert!(!refresh.inventory_complete);
    assert!(refresh.region_coverage_complete);
    assert_eq!(refresh.files_indexed, 0);
    assert_eq!(refresh.files_skipped, 1);
    assert!(refresh.truncated);
    assert_eq!(
        hierarchy
            .get_node("project-1", &file_id)
            .await
            .expect("file should reload")
            .expect("file should remain"),
        before
    );
    let hits = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "last_complete_generation".to_string(),
            max_results: 10,
        })
        .await
        .expect("last complete route should remain queryable")
        .data;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].freshness, ContextMapFreshness::Current);

    fs::write(
        root.path().join("recovered.md"),
        "newly_discovered_after_retry\n",
    )
    .expect("recovered source fixture should write");
    let recovered = indexer
        .refresh(request)
        .await
        .expect("a normal retry should complete");
    assert!(recovered.inventory_complete);
    assert!(recovered.region_coverage_complete);
    assert_eq!(recovered.files_indexed, 2);
    assert_eq!(recovered.files_skipped, 0);
    let recovered_status = hierarchy
        .project_intelligence_status("project-1")
        .await
        .expect("recovered project status should load")
        .last_refresh
        .expect("recovered refresh status should persist");
    assert!(recovered_status.inventory_complete);
    assert!(recovered_status.region_coverage_complete);
    assert_eq!(recovered_status.files_indexed, 2);
    assert_eq!(recovered_status.files_skipped, 0);
    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "newly_discovered_after_retry".to_string(),
                max_results: 10,
            })
            .await
            .expect("recovered source query should succeed")
            .data
            .len(),
        1
    );
}

#[tokio::test]
async fn older_paused_refresh_cannot_publish_after_newer_refresh_completes() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let source = root.path().join("facts.md");
    fs::write(&source, "older_refresh_fact\n").expect("older source fixture should write");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let older_indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    let newer_indexer = ProjectIndexer::new(
        HierarchyStore::open(&sqlite)
            .await
            .expect("second hierarchy store"),
        ContextMapStore::open(&sqlite)
            .await
            .expect("second context map store"),
    );
    let request = ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![root.path().to_path_buf()],
    };

    let older_generation = super::generation::claim(&older_indexer, "project-1")
        .await
        .expect("older refresh should claim a generation");
    let older_scan = super::scan::scan_roots(&request.roots).expect("older scan should complete");

    fs::write(&source, "newer_refresh_fact\n").expect("newer source fixture should write");
    newer_indexer
        .refresh(request.clone())
        .await
        .expect("newer refresh should complete");
    let completed_status = hierarchy
        .project_intelligence_status("project-1")
        .await
        .expect("newer project status should load")
        .last_refresh
        .expect("newer refresh status should persist");

    let stale = older_indexer
        .publish_refresh(request, older_scan, older_generation)
        .await
        .expect_err("older publisher should be rejected");
    assert!(matches!(stale, ProjectIndexerError::SupersededRefresh));
    assert_eq!(
        hierarchy
            .project_intelligence_status("project-1")
            .await
            .expect("project status should reload")
            .last_refresh
            .expect("newer refresh status should remain"),
        completed_status
    );
    assert!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "older_refresh_fact".to_string(),
                max_results: 10,
            })
            .await
            .expect("older route query should succeed")
            .data
            .is_empty()
    );
    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project-1".to_string(),
                text: "newer_refresh_fact".to_string(),
                max_results: 10,
            })
            .await
            .expect("newer route query should succeed")
            .data
            .len(),
        1
    );
}
