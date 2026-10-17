use std::fs;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;
use crate::ContextMapQuery;
use crate::ContextMapQueryResult;

#[tokio::test]
async fn private_storage_never_becomes_a_search_route() -> Result<(), Box<dyn std::error::Error>> {
    let home = TempDir::new()?;
    let root = TempDir::new()?;
    fs::write(root.path().join(".ignore"), "!.stateful/\n!.stateful/**\n")?;
    fs::create_dir_all(root.path().join(".stateful/exports"))?;
    fs::write(
        root.path().join(".stateful/exports/private.md"),
        "private_storage_canary",
    )?;
    fs::write(root.path().join("facts.md"), "ordinary_source_canary")?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await?;
    let context_map = ContextMapStore::open(&sqlite).await?;
    let indexer = ProjectIndexer::new(hierarchy, context_map.clone());
    let request = ProjectIndexRequest {
        project_id: "project".into(),
        roots: vec![root.path().to_path_buf()],
    };
    let report = indexer.refresh(request.clone()).await?;
    assert_eq!(
        (
            report.files_indexed,
            report.files_skipped,
            report.inventory_complete
        ),
        (1, 0, true)
    );
    let private_request = ProjectIndexFileRequest {
        project_id: "project".into(),
        project_root: root.path().to_path_buf(),
        relative_path: ProjectRelativePath::parse(".stateful/exports/private.md")?,
    };
    assert!(matches!(
        indexer.refresh_file(private_request).await,
        Err(ProjectIndexerError::SourceNotIndexed(_))
    ));
    let private_query = ContextMapQuery {
        project_id: "project".into(),
        text: "private_storage_canary".into(),
        max_results: 10,
    };
    assert_eq!(
        context_map.query(private_query).await?,
        ContextMapQueryResult {
            data: vec![],
            truncated: false
        }
    );
    let ordinary = context_map
        .query(ContextMapQuery {
            project_id: "project".into(),
            text: "ordinary_source_canary".into(),
            max_results: 10,
        })
        .await?;
    assert_eq!(ordinary.data.len(), 1);
    fs::write(
        root.path().join(".stateful/exports/private.md"),
        "changed_private_canary",
    )?;
    let refreshed = indexer.refresh(request).await?;
    assert_eq!(
        (
            refreshed.files_indexed,
            refreshed.missing_files,
            refreshed.inventory_complete
        ),
        (1, 0, true)
    );
    assert_eq!(
        context_map
            .query(ContextMapQuery {
                project_id: "project".into(),
                text: "ordinary_source_canary".into(),
                max_results: 10
            })
            .await?,
        ordinary
    );
    Ok(())
}

#[tokio::test]
async fn full_and_targeted_refresh_retire_files_and_regions_independently()
-> Result<(), Box<dyn std::error::Error>> {
    for targeted_first in [false, true] {
        let home = TempDir::new()?;
        let root = TempDir::new()?;
        let alias = root.path().join("alias");
        fs::create_dir(&alias)?;
        fs::write(
            alias.join("facts.md"),
            "historical_route_canary\n".repeat(/*n*/ 70),
        )?;
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let hierarchy = HierarchyStore::open(&sqlite).await?;
        let context_map = ContextMapStore::open(&sqlite).await?;
        let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
        let request = ProjectIndexRequest {
            project_id: "project".into(),
            roots: vec![root.path().to_path_buf()],
        };
        let targeted = ProjectIndexFileRequest {
            project_id: "project".into(),
            project_root: root.path().to_path_buf(),
            relative_path: ProjectRelativePath::parse("alias/facts.md")?,
        };
        indexer.refresh(request.clone()).await?;
        let file = context_map
            .file_hits_for_path("project", &targeted.relative_path)
            .await?
            .pop()
            .expect("actual FILE hit");
        let file_node = hierarchy
            .get_node("project", &file.entry.value.node_id)
            .await?
            .expect("file node");
        let regions = hierarchy.list_children("project", &file_node.id).await?;
        assert_eq!(regions.len(), 2);
        let query = ContextMapQuery {
            project_id: "project".into(),
            text: "historical_route_canary".into(),
            max_results: 10,
        };
        let initial = context_map.query(query.clone()).await?;
        assert_eq!(initial.data.len(), 2);
        let storage = root.path().join(".stateful");
        fs::rename(&alias, &storage)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&storage, &alias)?;
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&alias)
                .arg(&storage)
                .output()?;
            assert!(
                output.status.success(),
                "junction creation failed: {output:?}"
            );
        }
        let first = if targeted_first {
            indexer.refresh_file(targeted.clone()).await?
        } else {
            indexer.refresh(request.clone()).await?
        };
        assert_eq!((first.files_indexed, first.missing_files), (0, 1));
        let mut unavailable = file.clone();
        unavailable.freshness = crate::ContextMapFreshness::SourceUnavailable;
        assert_eq!(
            context_map.query(query).await?,
            ContextMapQueryResult {
                data: vec![unavailable.clone()],
                truncated: false
            }
        );
        assert_eq!(
            context_map.get_hit("project", &file.entry.id).await?,
            Some(unavailable)
        );
        for original in std::iter::once(file_node).chain(regions) {
            let missing = hierarchy
                .get_node("project", &original.id)
                .await?
                .expect("history retained");
            let mut expected = original;
            expected.revision += 1;
            expected.lifecycle = NodeLifecycle::Missing;
            expected.updated_at_ms = missing.updated_at_ms;
            assert_eq!(missing, expected);
        }
        for hit in initial.data {
            let mut expected = hit.clone();
            expected.freshness = crate::ContextMapFreshness::SourceUnavailable;
            assert_eq!(
                context_map.get_hit("project", &hit.entry.id).await?,
                Some(expected)
            );
        }
        let retired = hierarchy
            .list_children("project", &file.entry.value.node_id)
            .await?;
        let second = if targeted_first {
            indexer.refresh(request).await?
        } else {
            indexer.refresh_file(targeted).await?
        };
        assert_eq!((second.files_indexed, second.missing_files), (0, 0));
        assert_eq!(
            hierarchy
                .list_children("project", &file.entry.value.node_id)
                .await?,
            retired
        );
    }
    Ok(())
}
