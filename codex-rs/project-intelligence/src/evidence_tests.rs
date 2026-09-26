use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ContextMapStore;
use crate::HierarchyStore;
use crate::ProjectIndexFileRequest;
use crate::ProjectIndexReport;
use crate::ProjectIndexRequest;
use crate::ProjectIndexer;

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
    assert!(reader.read(request).await.is_err());

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
