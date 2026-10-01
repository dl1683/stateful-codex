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
    // Reindexing rebinds the entry to the new bytes, so the old route is obsolete and
    // fails its fingerprint guard; it is never silently moved to new coordinates.
    assert!(matches!(
        reader.read(request).await,
        Err(EvidenceReadError::RouteFingerprintMismatch)
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
async fn guarded_route_failures_name_their_actual_cause() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    std::fs::write(
        root.path().join("a.txt"),
        "alpha
",
    )
    .expect("write fixture");
    std::fs::write(
        root.path().join("b.txt"),
        "beta
",
    )
    .expect("write fixture");
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
    let mut routes = Vec::new();
    for path in ["a.txt", "b.txt"] {
        let hit = context_map
            .file_hits_for_path(
                "project-1",
                &ProjectRelativePath::parse(path).expect("relative path"),
            )
            .await
            .expect("source lookup")
            .into_iter()
            .next()
            .expect("indexed source");
        routes.push(EvidenceRoute::from_hit(&hit).expect("file route"));
    }
    let (a, b) = (routes[0].clone(), routes[1].clone());
    let reader = EvidenceReader::new(context_map);
    let read = |project_id: &str, route: EvidenceRoute| {
        reader.read(EvidenceReadRequest {
            project_id: project_id.to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route),
            max_bytes: 64,
        })
    };
    let outcome = |result: Result<EvidenceReadResult, EvidenceReadError>| match result {
        Ok(read) => format!("read {:?}", read.content),
        Err(error) => format!("{error:?}"),
    };

    let mut observed = vec![
        ("current", outcome(read("project-1", a.clone()).await)),
        (
            "unknown id",
            outcome(
                read(
                    "project-1",
                    EvidenceRoute {
                        context_map_entry_id: ContextMapEntryId::parse("missing-route")
                            .expect("valid entry ID"),
                        ..a.clone()
                    },
                )
                .await,
            ),
        ),
        ("wrong project", outcome(read("project-2", a.clone()).await)),
        (
            "altered fingerprint",
            outcome(
                read(
                    "project-1",
                    EvidenceRoute {
                        source_fingerprint: SourceFingerprint::parse(format!(
                            "sha256:{}",
                            "0".repeat(/*n*/ 64)
                        ))
                        .expect("valid fingerprint"),
                        ..a.clone()
                    },
                )
                .await,
            ),
        ),
        (
            "mispaired fingerprint",
            outcome(
                read(
                    "project-1",
                    EvidenceRoute {
                        source_fingerprint: b.source_fingerprint.clone(),
                        ..a.clone()
                    },
                )
                .await,
            ),
        ),
        (
            "altered range",
            outcome(
                read(
                    "project-1",
                    EvidenceRoute {
                        line_range: Some(EvidenceLineRange { start: 1, end: 1 }),
                        ..a.clone()
                    },
                )
                .await,
            ),
        ),
    ];
    std::fs::write(
        root.path().join("a.txt"),
        "changed
",
    )
    .expect("change source");
    std::fs::remove_file(root.path().join("b.txt")).expect("remove source");
    observed.push(("changed bytes", outcome(read("project-1", a).await)));
    observed.push(("removed source", outcome(read("project-1", b).await)));

    assert_eq!(
        observed,
        vec![
            ("current", r#"read "alpha\n""#.to_string()),
            ("unknown id", "RouteNotFound(\"missing-route\")".to_string()),
            (
                "wrong project",
                format!(
                    "RouteNotFound({:?})",
                    routes[0].context_map_entry_id.to_string()
                )
            ),
            (
                "altered fingerprint",
                "RouteFingerprintMismatch".to_string()
            ),
            (
                "mispaired fingerprint",
                "RouteFingerprintMismatch".to_string()
            ),
            ("altered range", "RouteChanged".to_string()),
            ("changed bytes", "SourceChanged".to_string()),
            (
                "removed source",
                "SourceNotCurrent(SourceUnavailable)".to_string()
            ),
        ]
    );
}

#[tokio::test]
async fn region_route_reports_stale_index_separately_from_a_removed_file() {
    let home = TempDir::new().expect("temporary state home");
    let root = TempDir::new().expect("temporary project root");
    let source = (1..=70)
        .map(|line| format!("region_fact line {line}\n"))
        .collect::<String>();
    std::fs::write(root.path().join("facts.txt"), source).expect("write fixture");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
    let index = ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![root.path().to_path_buf()],
    };
    indexer.refresh(index.clone()).await.expect("index project");
    let region = context_map
        .query(crate::ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "region_fact".to_string(),
            max_results: 10,
        })
        .await
        .expect("query route")
        .data
        .into_iter()
        .find(|hit| hit.source.region_anchor.is_some())
        .expect("indexed region");
    let route = EvidenceRoute::from_hit(&region).expect("region route");
    let reader = EvidenceReader::new(context_map);
    let read = || {
        reader.read(EvidenceReadRequest {
            project_id: "project-1".to_string(),
            project_roots: vec![root.path().to_path_buf()],
            locator: EvidenceReadLocator::ContextMapRoute(route.clone()),
            max_bytes: 64,
        })
    };

    // The parent file advances while the region entry keeps the issued fingerprint.
    let region_node = hierarchy
        .get_node("project-1", &region.entry.value.node_id)
        .await
        .expect("region node lookup")
        .expect("region node");
    let file_id = region_node.value.parent_id.expect("region parent");
    let file = hierarchy
        .get_node("project-1", &file_id)
        .await
        .expect("file node lookup")
        .expect("file node");
    hierarchy
        .update_source_state(
            "project-1",
            &file_id,
            crate::HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: crate::NodeLifecycle::Active,
                source_fingerprint: Some(
                    SourceFingerprint::parse(format!("sha256:{}", "1".repeat(/*n*/ 64)))
                        .expect("valid fingerprint"),
                ),
            },
        )
        .await
        .expect("advance parent state");
    let stale = format!("{:?}", read().await.err());

    std::fs::remove_file(root.path().join("facts.txt")).expect("remove source");
    indexer.refresh(index).await.expect("reindex project");
    let removed = format!("{:?}", read().await.err());

    assert_eq!(
        (stale, removed),
        (
            "Some(SourceNotCurrent(Stale))".to_string(),
            "Some(SourceNotCurrent(SourceUnavailable))".to_string(),
        )
    );
}
