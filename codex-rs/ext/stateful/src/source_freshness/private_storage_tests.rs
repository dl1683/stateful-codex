use std::fs;

use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

#[tokio::test]
async fn native_audits_and_refreshes_retire_all_private_source_lifecycles()
-> Result<(), Box<dyn std::error::Error>> {
    for (targeted_first, stale_first) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let home = TempDir::new()?;
        let root = TempDir::new()?;
        let alias = root.path().join("alias");
        fs::create_dir(&alias)?;
        fs::write(
            alias.join("facts.md"),
            "unchanged_historical_bytes\n".repeat(/*n*/ 70),
        )?;
        let services =
            ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
        let hierarchy = services.hierarchy().await?;
        let context_map = services.context_map().await?;
        let indexer = ProjectIndexer::new(hierarchy.clone(), context_map.clone());
        let request = ProjectIndexRequest {
            project_id: "project".into(),
            roots: vec![root.path().to_path_buf()],
        };
        indexer.refresh(request.clone()).await?;
        let relative_path = ProjectRelativePath::parse("alias/facts.md")?;
        let hit = context_map
            .file_hits_for_path("project", &relative_path)
            .await?
            .pop()
            .expect("FILE route");
        let region_hits = context_map
            .query(codex_project_intelligence::ContextMapQuery {
                project_id: "project".into(),
                text: "unchanged_historical_bytes".into(),
                max_results: 10,
            })
            .await?
            .data;
        assert_eq!(region_hits.len(), 2);
        let ids = std::iter::once(hit.entry.id.clone())
            .chain(region_hits.iter().map(|hit| hit.entry.id.clone()))
            .collect::<Vec<_>>();
        let roots = [root.path().to_path_buf()];
        if stale_first {
            fs::write(
                alias.join("facts.md"),
                "changed_source_bytes\n".repeat(/*n*/ 70),
            )?;
            let audit = audit_root_evidence(
                &services,
                "project",
                &roots,
                ids.clone(),
                /*cache_key*/ None,
            )
            .await;
            assert!(audit.hashed_bytes > 0);
            assert_eq!(
                audit.statuses,
                ids.iter()
                    .cloned()
                    .map(|id| (id, SourceAuditStatus::Stale))
                    .collect()
            );
        }
        let before = hierarchy
            .get_node("project", &hit.entry.value.node_id)
            .await?
            .expect("file node");
        let regions = hierarchy.list_children("project", &before.id).await?;
        assert_eq!(regions.len(), 2);
        let lifecycle = if stale_first {
            NodeLifecycle::Replaced
        } else {
            NodeLifecycle::Active
        };
        assert_eq!(
            std::iter::once(before.lifecycle)
                .chain(regions.iter().map(|node| node.lifecycle))
                .collect::<Vec<_>>(),
            vec![lifecycle; 3]
        );
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
        if !stale_first {
            let audit = audit_root_evidence(
                &services,
                "project",
                &roots,
                [hit.entry.id.clone()],
                /*cache_key*/ None,
            )
            .await;
            assert_eq!(
                (audit.hashed_bytes, audit.statuses),
                (
                    0,
                    HashMap::from([(hit.entry.id.clone(), SourceAuditStatus::SourceUnavailable)])
                )
            );
            // FILE audit precedes refresh; its active children need repair even
            // though the parent is already missing. The stale-first cases keep
            // both file and regions Replaced until exclusion refresh.
            assert_eq!(
                hierarchy.list_children("project", &before.id).await?,
                regions
            );
        }
        let report = if targeted_first {
            indexer
                .refresh_file(codex_project_intelligence::ProjectIndexFileRequest {
                    project_id: "project".into(),
                    project_root: root.path().to_path_buf(),
                    relative_path,
                })
                .await?
        } else {
            indexer.refresh(request).await?
        };
        assert_eq!(
            (report.files_indexed, report.missing_files),
            (0, u64::from(stale_first))
        );
        let missing = hierarchy
            .get_node("project", &before.id)
            .await?
            .expect("retained file");
        let mut expected = before;
        expected.revision += 1;
        expected.lifecycle = NodeLifecycle::Missing;
        expected.updated_at_ms = missing.updated_at_ms;
        assert_eq!(missing, expected);
        let retired = hierarchy.list_children("project", &missing.id).await?;
        for (original, actual) in regions.into_iter().zip(&retired) {
            let mut expected = original;
            expected.revision += 1;
            expected.lifecycle = NodeLifecycle::Missing;
            expected.updated_at_ms = actual.updated_at_ms;
            assert_eq!(*actual, expected);
        }
        let repeated = audit_root_evidence(
            &services,
            "project",
            &roots,
            ids.clone(),
            /*cache_key*/ None,
        )
        .await;
        assert_eq!(
            (repeated.hashed_bytes, repeated.statuses),
            (
                0,
                ids.into_iter()
                    .map(|id| (id, SourceAuditStatus::SourceUnavailable))
                    .collect()
            )
        );
        assert_eq!(
            hierarchy.get_node("project", &missing.id).await?,
            Some(missing)
        );
        assert_eq!(
            hierarchy.list_children("project", &expected.id).await?,
            retired
        );
        for original in std::iter::once(hit).chain(region_hits) {
            let actual = context_map
                .get_hit("project", &original.entry.id)
                .await?
                .expect("historical hit");
            let mut expected = original;
            expected.freshness = codex_project_intelligence::ContextMapFreshness::SourceUnavailable;
            assert_eq!(actual, expected);
        }
        assert!(matches!(
            fingerprint_source(root.path(), ".stateful/facts.md", MAX_AUDITED_SOURCE_BYTES),
            Err(SourceCheckError::Missing)
        ));
    }
    Ok(())
}
