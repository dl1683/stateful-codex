use std::future::Future;
use std::io::Cursor;
use std::io::Write;
use std::task::Poll;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use zip::CompressionMethod;
use zip::DateTime;
use zip::ZipArchive;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::ContextMapStore;
use crate::HierarchyRegionSourceUpdate;
use crate::HierarchyStore;
use crate::NodeLifecycle;
use crate::ProjectIndexFileRequest;
use crate::ProjectIndexReport;
use crate::ProjectIndexRequest;
use crate::ProjectIndexer;
use crate::RegionAnchor;

fn generated_docx(body: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0)
        .last_modified_time(DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).expect("date"));
    writer
        .start_file("word/document.xml", options)
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

async fn indexed_docx() -> (TempDir, TempDir, ContextMapStore, ContextMapHit) {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    std::fs::write(
        root.path().join("fixture.docx"),
        generated_docx(
            r#"<w:p><w:r><w:t>exact region content</w:t></w:r></w:p><w:p><w:r><w:t>another indexed clause</w:t></w:r></w:p>"#,
        ),
    )
    .expect("write DOCX fixture");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    ProjectIndexer::new(
        HierarchyStore::open(&sqlite).await.expect("hierarchy"),
        context_map.clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![root.path().to_path_buf()],
    })
    .await
    .expect("index DOCX project");
    let hit = context_map
        .query(crate::ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "exact region content".to_string(),
            max_results: 10,
        })
        .await
        .expect("query DOCX region")
        .data
        .into_iter()
        .find(|hit| hit.source.region_anchor.is_some())
        .expect("DOCX region");
    (home, root, context_map, hit)
}

fn utf8_valid_docx() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = ZipWriter::new(Cursor::new(&mut bytes));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0)
        .last_modified_time(DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).expect("date"));
    writer
        .start_file("word/document.xml", options)
        .expect("document part should start");
    writer.finish().expect("package should finish");
    let central = bytes
        .windows(4)
        .position(|signature| signature == b"PK\x01\x02")
        .expect("central directory");
    bytes[central + 38..central + 42].fill(0);
    assert!(
        std::str::from_utf8(&bytes).is_ok(),
        "UTF-8 error: {:?}",
        std::str::from_utf8(&bytes).expect_err("fixture should be UTF-8")
    );
    bytes
}

async fn indexed_utf8_docx() -> (TempDir, TempDir, ContextMapStore, ContextMapHit) {
    indexed_utf8_docx_named("fixture.docx").await
}

async fn indexed_utf8_docx_named(name: &str) -> (TempDir, TempDir, ContextMapStore, ContextMapHit) {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    std::fs::write(root.path().join(name), utf8_valid_docx())
        .expect("write UTF-8-valid DOCX fixture");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    ProjectIndexer::new(
        HierarchyStore::open(&sqlite).await.expect("hierarchy"),
        context_map.clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![root.path().to_path_buf()],
    })
    .await
    .expect("index UTF-8-valid DOCX project");
    let hit = context_map
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse(name).expect("relative path"),
        )
        .await
        .expect("file hit")
        .into_iter()
        .next()
        .expect("UTF-8-valid DOCX file");
    (home, root, context_map, hit)
}

#[tokio::test]
async fn reads_a_fingerprint_verified_line_range_from_the_indexed_source() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let source = root.path().join("evidence.txt");
    std::fs::write(&source, "alpha\nbeta\ngamma\ndelta\n").expect("write fixture");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    ProjectIndexer::new(
        HierarchyStore::open(&sqlite).await.expect("hierarchy"),
        context_map.clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![root.path().to_path_buf()],
    })
    .await
    .expect("index project");
    let relative_path = ProjectRelativePath::parse("evidence.txt").expect("relative path");
    let hit = context_map
        .file_hits_for_path("project-1", &relative_path)
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let reader = EvidenceReader::new(context_map);
    let result = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::Source {
                project_root: None,
                relative_path: relative_path.clone(),
                line_range: Some(EvidenceLineRange { start: 2, end: 3 }),
            },
            max_bytes: 64,
        })
        .await
        .expect("read exact lines");

    assert_eq!(
        result,
        EvidenceReadResult {
            hit,
            content: "beta\ngamma\n".to_string(),
            bytes_returned: 11,
            total_bytes: 23,
            total_lines: 4,
            first_line: Some(2),
            last_line: Some(3),
            truncated: false,
            extraction: None,
        }
    );

    std::fs::write(source, "alpha\nchanged\ngamma\ndelta\n").expect("change source");
    let error = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::Source {
                project_root: None,
                relative_path,
                line_range: Some(EvidenceLineRange { start: 2, end: 3 }),
            },
            max_bytes: 64,
        })
        .await
        .expect_err("changed source must not support evidence");
    assert!(matches!(error, EvidenceReadError::SourceChanged));

    let report = ProjectIndexer::new(
        HierarchyStore::open(&sqlite).await.expect("hierarchy"),
        reader.context_map.clone(),
    )
    .refresh_file(ProjectIndexFileRequest {
        project_id: "project-1".to_string(),
        project_root: root.path().to_path_buf(),
        relative_path: ProjectRelativePath::parse("evidence.txt").expect("relative path"),
    })
    .await
    .expect("refresh changed source");
    let mut stable_report = report;
    stable_report.scan_duration_ms = 0;
    stable_report.publication_duration_ms = 0;
    assert_eq!(
        stable_report,
        ProjectIndexReport {
            inventory_complete: true,
            region_coverage_complete: true,
            files_indexed: 1,
            regions_indexed: 1,
            files_skipped: 0,
            missing_files: 0,
            truncated: false,
            scan_duration_ms: 0,
            publication_duration_ms: 0,
        }
    );
    let refreshed = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::Source {
                project_root: None,
                relative_path: ProjectRelativePath::parse("evidence.txt").expect("relative path"),
                line_range: Some(EvidenceLineRange { start: 2, end: 3 }),
            },
            max_bytes: 64,
        })
        .await
        .expect("read refreshed source");
    assert_eq!(refreshed.content, "changed\ngamma\n");
}

#[tokio::test]
async fn guarded_region_route_rejects_shifted_coordinates_until_requeried() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let source = root.path().join("facts.txt");
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
    std::fs::write(&source, &initial).expect("write fixture");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let indexer = ProjectIndexer::new(
        HierarchyStore::open(&sqlite).await.expect("hierarchy"),
        context_map.clone(),
    );
    indexer
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![root.path().to_path_buf()],
        })
        .await
        .expect("index project");
    let hit = context_map
        .query(crate::ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "decisive_route_fact".to_string(),
            max_results: 10,
        })
        .await
        .expect("query route")
        .data
        .into_iter()
        .next()
        .expect("decisive region");
    let route = EvidenceRoute::from_hit(&hit).expect("guarded evidence route");
    assert!(route.line_range.is_some());
    assert_eq!(route.region_anchor, None);
    assert_eq!(route.indexed_extraction, None);
    let reader = EvidenceReader::new(context_map.clone());
    let request = EvidenceReadRequest {
        project_id: "project-1".to_string(),
        project_roots: vec![root.path().to_path_buf()],
        locator: EvidenceReadLocator::ContextMapRoute(route.clone()),
        max_bytes: 1024,
    };
    let read = reader
        .read(request.clone())
        .await
        .expect("current route should read");
    assert!(read.content.ends_with("decisive_route_fact"));

    std::fs::write(&source, format!("inserted\n{initial}")).expect("shift source coordinates");
    assert!(matches!(
        reader.read(request.clone()).await,
        Err(EvidenceReadError::SourceChanged)
    ));

    indexer
        .refresh_file(ProjectIndexFileRequest {
            project_id: "project-1".to_string(),
            project_root: root.path().to_path_buf(),
            relative_path: ProjectRelativePath::parse("facts.txt").expect("relative path"),
        })
        .await
        .expect("refresh shifted source");
    assert!(matches!(
        reader.read(request).await,
        Err(EvidenceReadError::RouteChanged)
    ));

    let current_hit = context_map
        .query(crate::ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "decisive_route_fact".to_string(),
            max_results: 10,
        })
        .await
        .expect("query current route")
        .data
        .into_iter()
        .next()
        .expect("current decisive region");
    let current = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(
                EvidenceRoute::from_hit(&current_hit).expect("current guarded route"),
            ),
            max_bytes: 1024,
        })
        .await
        .expect("requeried route should read");
    assert!(current.content.ends_with("decisive_route_fact"));
    assert_eq!(current.last_line, Some(71));
}

#[tokio::test]
async fn reads_exact_docx_region_with_provenance_and_budget_truncation() {
    let (_home, root, context_map, hit) = indexed_docx().await;
    let route = EvidenceRoute::from_hit(&hit).expect("DOCX route");
    assert_eq!(route.line_range, None);
    assert!(route.region_anchor.is_some());
    assert!(route.indexed_extraction.is_some());
    let reader = EvidenceReader::new(context_map);
    let result = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route.clone()),
            max_bytes: 64 * 1024,
        })
        .await
        .expect("read exact DOCX region");
    assert_eq!(result.content, "exact region content");
    assert_eq!(result.extraction, hit.source.indexed_extraction);
    assert!(!result.truncated);
    assert_eq!(result.first_line, None);
    assert_eq!(result.last_line, None);

    let partial = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route),
            max_bytes: 5,
        })
        .await
        .expect("bounded DOCX region read");
    assert_eq!(partial.content, "exact");
    assert!(partial.truncated);
}

#[tokio::test]
async fn third_office_read_waits_until_a_permit_is_released() {
    let (_home, root, context_map, hit) = indexed_docx().await;
    let route = EvidenceRoute::from_hit(&hit).expect("DOCX route");
    let permits = OFFICE_READS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_OFFICE_READS)))
        .clone()
        .acquire_many_owned(MAX_CONCURRENT_OFFICE_READS as u32)
        .await
        .expect("all Office permits should be available");
    let reader = EvidenceReader::new(context_map);
    let mut read = Box::pin(reader.read(EvidenceReadRequest {
        project_id: "project-1".to_string(),
        project_roots: vec![root.path().to_path_buf()],
        locator: EvidenceReadLocator::ContextMapRoute(route),
        max_bytes: 64 * 1024,
    }));
    let pending =
        std::future::poll_fn(|context| Poll::Ready(read.as_mut().poll(context).is_pending())).await;
    assert!(pending, "third Office read should wait for a permit");
    drop(permits);
    read.await
        .expect("Office read should proceed after release");
}

#[tokio::test]
async fn office_file_routes_are_rejected_before_raw_utf8_reads() {
    let (_home, root, context_map, hit) = indexed_utf8_docx().await;
    let bytes = std::fs::read(root.path().join("fixture.docx")).expect("read DOCX fixture");
    assert!(std::str::from_utf8(&bytes).is_ok());
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("stored ZIP fixture");
    assert_eq!(
        archive.by_index(0).expect("document part").compression(),
        CompressionMethod::Stored
    );
    let file_hit = context_map
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse("fixture.docx").expect("relative path"),
        )
        .await
        .expect("file hit")
        .into_iter()
        .next()
        .expect("DOCX file hit");
    let reader = EvidenceReader::new(context_map);
    let source_error = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::Source {
                project_root: None,
                relative_path: ProjectRelativePath::parse("fixture.docx").expect("relative path"),
                line_range: None,
            },
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("relativePath DOCX reads must be rejected");
    assert!(matches!(source_error, EvidenceReadError::OfficeSourceRoute));

    let route = EvidenceRoute::from_hit(&file_hit).expect("file route");
    assert_eq!(route.region_anchor, None);
    let route_error = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route),
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("file-node DOCX routes must be rejected");
    assert!(matches!(route_error, EvidenceReadError::OfficeSourceRoute));

    assert_eq!(hit.source.region_anchor, None);
}

#[tokio::test]
async fn renamed_office_file_routes_are_rejected_before_raw_utf8_reads() {
    let (_home, root, context_map, hit) = indexed_utf8_docx_named("fixture.txt").await;
    let reader = EvidenceReader::new(context_map);
    for line_range in [None, Some(EvidenceLineRange { start: 1, end: 1 })] {
        let error = reader
            .read(EvidenceReadRequest {
                project_id: "project-1".to_string(),
                project_roots: vec![root.path().to_path_buf()],
                locator: EvidenceReadLocator::Source {
                    project_root: None,
                    relative_path: ProjectRelativePath::parse("fixture.txt")
                        .expect("relative path"),
                    line_range,
                },
                max_bytes: 64 * 1024,
            })
            .await
            .expect_err("renamed Office source reads must be rejected");
        assert!(matches!(error, EvidenceReadError::OfficeSourceRoute));
    }

    let route = EvidenceRoute::from_hit(&hit).expect("renamed Office file route");
    let error = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route),
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("renamed Office file routes must be rejected");
    assert!(matches!(error, EvidenceReadError::OfficeSourceRoute));
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn in_root_symlink_to_renamed_office_is_rejected() {
    let (home, root, context_map, _) = indexed_utf8_docx_named("target.txt").await;
    let link = root.path().join("link.txt");
    let target = root.path().join("target.txt");
    let symlink_result = {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(&target, &link)
        }
    };
    if symlink_result.is_err() {
        return;
    }
    ProjectIndexer::new(
        HierarchyStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
            .await
            .expect("hierarchy"),
        context_map.clone(),
    )
    .refresh_file(ProjectIndexFileRequest {
        project_id: "project-1".to_string(),
        project_root: root.path().to_path_buf(),
        relative_path: ProjectRelativePath::parse("link.txt").expect("link path"),
    })
    .await
    .expect("symlink should be indexed");
    let hit = context_map
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse("link.txt").expect("link path"),
        )
        .await
        .expect("symlink file hit")
        .into_iter()
        .next()
        .expect("symlink file route");
    let error = EvidenceReader::new(context_map)
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(
                EvidenceRoute::from_hit(&hit).expect("symlink route"),
            ),
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("symlink Office file route must be rejected");
    assert!(matches!(error, EvidenceReadError::OfficeSourceRoute));
}

#[tokio::test]
async fn docx_region_source_mutation_is_source_changed() {
    let (_home, root, context_map, hit) = indexed_docx().await;
    let route = EvidenceRoute::from_hit(&hit).expect("DOCX route");
    std::fs::write(
        root.path().join("fixture.docx"),
        generated_docx(r#"<w:p><w:r><w:t>mutated region content</w:t></w:r></w:p>"#),
    )
    .expect("mutate DOCX fixture");
    let error = EvidenceReader::new(context_map)
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route),
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("mutated DOCX must fail closed");
    assert!(matches!(error, EvidenceReadError::SourceChanged));
}

#[tokio::test]
async fn unknown_docx_anchor_fails_closed() {
    let (home, root, context_map, hit) = indexed_docx().await;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let region = hierarchy
        .get_node("project-1", &hit.entry.value.node_id)
        .await
        .expect("load region")
        .expect("region");
    hierarchy
        .update_region_source(
            "project-1",
            &region.id,
            HierarchyRegionSourceUpdate {
                expected_revision: region.revision,
                lifecycle: NodeLifecycle::Active,
                region_anchor: RegionAnchor::new("docx-paragraph", "body/p[999]")
                    .expect("valid unknown anchor"),
                source_fingerprint: hit.entry.value.source_fingerprint.clone(),
            },
        )
        .await
        .expect("store unknown anchor");
    let route = EvidenceRoute::from_hit(
        &context_map
            .get_hit("project-1", &hit.entry.id)
            .await
            .expect("load changed route")
            .expect("changed route"),
    )
    .expect("DOCX route");
    let error = EvidenceReader::new(context_map)
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route),
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("unknown route must fail closed");
    assert!(
        matches!(error, EvidenceReadError::RegionAnchorNotFound),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn extractor_upgrade_requires_refresh_and_requery() {
    let (home, root, context_map, hit) = indexed_docx().await;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let pool = sqlite
        .open_read_write_pool(&home.path().join("project_intelligence_1.sqlite"))
        .await
        .expect("project intelligence database");
    sqlx::query(
        "UPDATE region_extraction_attestations
         SET extractor_version = '1'
         WHERE region_node_id = ?",
    )
    .bind(hit.entry.value.node_id.as_str())
    .execute(&pool)
    .await
    .expect("write v1 attestation");
    pool.close().await;
    let stored_v1 = context_map
        .get_hit("project-1", &hit.entry.id)
        .await
        .expect("load v1 hit")
        .expect("v1 hit");
    let v1_route = EvidenceRoute::from_hit(&stored_v1).expect("v1 route");
    let reader = EvidenceReader::new(context_map.clone());
    let error = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(v1_route.clone()),
            max_bytes: 64 * 1024,
        })
        .await
        .expect_err("extractor upgrade must fail closed");
    assert!(matches!(error, EvidenceReadError::ExtractionChanged));

    ProjectIndexer::new(
        HierarchyStore::open(&sqlite).await.expect("hierarchy"),
        context_map.clone(),
    )
    .refresh_file(ProjectIndexFileRequest {
        project_id: "project-1".to_string(),
        project_root: root.path().to_path_buf(),
        relative_path: ProjectRelativePath::parse("fixture.docx").expect("relative path"),
    })
    .await
    .expect("refresh extractor identity");
    assert!(matches!(
        reader
            .read(EvidenceReadRequest {
                project_id: "project-1".to_string(),
                project_roots: vec![root.path().to_path_buf()],
                locator: EvidenceReadLocator::ContextMapRoute(v1_route),
                max_bytes: 64 * 1024,
            })
            .await,
        Err(EvidenceReadError::RouteChanged)
    ));
    let current_hit = context_map
        .query(crate::ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "exact region content".to_string(),
            max_results: 10,
        })
        .await
        .expect("requery current route")
        .data
        .into_iter()
        .find(|hit| hit.source.region_anchor.is_some())
        .expect("current DOCX region");
    let current = reader
        .read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(
                EvidenceRoute::from_hit(&current_hit).expect("current route"),
            ),
            max_bytes: 64 * 1024,
        })
        .await
        .expect("new extractor route should read");
    assert_eq!(current.content, "exact region content");
}
