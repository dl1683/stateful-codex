use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server::INVALID_PARAMS_ERROR_CODE;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapQueryParams;
use codex_app_server_protocol::ContextMapQueryResponse;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::EvidenceEncoding;
use codex_app_server_protocol::EvidenceLineRange;
use codex_app_server_protocol::EvidenceReadParams;
use codex_app_server_protocol::EvidenceReadResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectIntelligenceNodeKind;
use codex_app_server_protocol::ProjectIntelligenceStatusParams;
use codex_app_server_protocol::ProjectIntelligenceStatusResponse;
use codex_app_server_protocol::ProjectIntelligenceTreeParams;
use codex_app_server_protocol::ProjectIntelligenceTreeResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_features::Feature;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn project_status_and_evidence_read_report_only_current_exact_source() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let source_path = project_root.path().join("EVIDENCE.txt");
    std::fs::write(&source_path, "decisive=42\nnext")?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let codex_home_path = AbsolutePathBuf::try_from(codex_home.path().to_path_buf())?;
    let project_root_path = AbsolutePathBuf::try_from(project_root.path().to_path_buf())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Evidence project".to_string(),
                roots: vec![ProjectRoot {
                    path: project_root_path.clone(),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "evidence-project".to_string(),
            },
        })
        .await?;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    assert_eq!(refreshed.missing_files, 0);

    let status: ProjectIntelligenceStatusResponse = server
        .request(|request_id| ClientRequest::ProjectIntelligenceStatus {
            request_id,
            params: ProjectIntelligenceStatusParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(status.project_id, created.project.id);
    assert_eq!(status.roots, vec![project_root_path.clone()]);
    assert!(status.initialized);
    assert_eq!(status.hierarchy_node_count, 4);
    assert_eq!(status.file_count, 1);
    assert_eq!(status.missing_source_count, 0);
    assert_eq!(status.context_map_entry_count, 2);
    assert_eq!(status.blackboard_entry_count, 0);
    assert_eq!(status.promoted_entry_count, 0);
    assert!(status.updated_at.is_some());

    let first_tree_page: ProjectIntelligenceTreeResponse = server
        .request(|request_id| ClientRequest::ProjectIntelligenceTree {
            request_id,
            params: ProjectIntelligenceTreeParams {
                project_id: status.project_id.clone(),
                cursor: None,
                limit: Some(2),
            },
        })
        .await?;
    let second_tree_page: ProjectIntelligenceTreeResponse = server
        .request(|request_id| ClientRequest::ProjectIntelligenceTree {
            request_id,
            params: ProjectIntelligenceTreeParams {
                project_id: status.project_id.clone(),
                cursor: first_tree_page.next_cursor.clone(),
                limit: Some(2),
            },
        })
        .await?;
    assert_eq!(first_tree_page.next_cursor, Some("2".to_string()));
    assert_eq!(second_tree_page.next_cursor, None);
    assert_eq!(
        first_tree_page
            .data
            .iter()
            .chain(&second_tree_page.data)
            .map(|node| (node.kind, node.relative_path.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (ProjectIntelligenceNodeKind::Project, ""),
            (ProjectIntelligenceNodeKind::Directory, ""),
            (ProjectIntelligenceNodeKind::File, "EVIDENCE.txt"),
            (ProjectIntelligenceNodeKind::Region, "EVIDENCE.txt"),
        ]
    );

    let route = ContextMapStore::open(&SqliteConfig::new_for_testing(codex_home_path))
        .await?
        .file_hits_for_path(
            &status.project_id,
            &ProjectRelativePath::parse("EVIDENCE.txt")?,
        )
        .await?
        .into_iter()
        .find(|route| route.source.region_anchor.is_none())
        .expect("indexed file source route");
    let evidence: EvidenceReadResponse = server
        .request(|request_id| ClientRequest::EvidenceRead {
            request_id,
            params: EvidenceReadParams {
                project_id: status.project_id.clone(),
                context_map_entry_id: route.entry.id.to_string(),
                line_range: Some(EvidenceLineRange { start: 2, end: 2 }),
                max_bytes: Some(12),
            },
        })
        .await?;
    assert_eq!(
        evidence,
        EvidenceReadResponse {
            project_id: status.project_id.clone(),
            context_map_entry_id: route.entry.id.to_string(),
            node_id: route.entry.value.node_id.to_string(),
            source_fingerprint: route.entry.value.source_fingerprint.to_string(),
            source: codex_app_server_protocol::ContextMapSource {
                project_root: project_root_path,
                relative_path: route.source.relative_path.to_string(),
                region_anchor: None,
            },
            encoding: EvidenceEncoding::Utf8,
            content: "next".to_string(),
            bytes_returned: 4,
            total_bytes: 16,
            total_lines: 2,
            first_line: Some(2),
            last_line: Some(2),
            truncated: false,
            revision: route.entry.revision,
        }
    );

    std::fs::write(source_path, "decisive=99\nnext")?;
    let request_id = server
        .send_request(
            "evidence/read",
            Some(json!({
                "projectId": status.project_id,
                "contextMapEntryId": route.entry.id,
                "maxBytes": 12,
            })),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request_id))
        .await?;
    assert_eq!(error.error.code, INVALID_PARAMS_ERROR_CODE);
    assert_eq!(
        error.error.message,
        "evidence source changed after indexing; refresh the context map"
    );
    Ok(())
}

#[tokio::test]
async fn evidence_read_accepts_current_exact_region_and_rejects_mismatched_range() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let source = (1..=70)
        .map(|line| {
            if line == 70 {
                "decisive_region_fact".to_string()
            } else {
                format!("line {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(project_root.path().join("FACTS.md"), source)?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Region evidence project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "region-evidence-project".to_string(),
            },
        })
        .await?;
    server
        .request::<ContextMapRefreshResponse>(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    let routes: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery {
            request_id,
            params: ContextMapQueryParams {
                project_id: created.project.id.clone(),
                text: "decisive_region_fact".to_string(),
                limit: Some(5),
            },
        })
        .await?;
    let route = routes
        .data
        .into_iter()
        .find(|route| route.source.region_anchor.is_some())
        .expect("query should return an exact region route");
    assert_eq!(
        route
            .source
            .region_anchor
            .as_ref()
            .map(|anchor| (anchor.scheme.as_str(), anchor.locator.as_str())),
        Some(("lines", "65-70"))
    );

    let evidence: EvidenceReadResponse = server
        .request(|request_id| ClientRequest::EvidenceRead {
            request_id,
            params: EvidenceReadParams {
                project_id: created.project.id.clone(),
                context_map_entry_id: route.entry_id.clone(),
                line_range: Some(EvidenceLineRange { start: 65, end: 70 }),
                max_bytes: Some(32_768),
            },
        })
        .await?;
    assert_eq!(evidence.context_map_entry_id, route.entry_id);
    assert_eq!(evidence.node_id, route.node_id);
    assert_eq!(evidence.source_fingerprint, route.source_fingerprint);
    assert_eq!(evidence.source, route.source);
    assert_eq!(
        (evidence.first_line, evidence.last_line),
        (Some(65), Some(70))
    );
    assert!(evidence.content.ends_with("decisive_region_fact"));

    let mismatched_request_id = server
        .send_request(
            "evidence/read",
            Some(json!({
                "projectId": created.project.id,
                "contextMapEntryId": evidence.context_map_entry_id,
                "lineRange": {"start": 64, "end": 70},
                "maxBytes": 32_768
            })),
        )
        .await?;
    let mismatch = server
        .read_stream_until_error_message(RequestId::Integer(mismatched_request_id))
        .await?;
    assert_eq!(mismatch.error.code, INVALID_PARAMS_ERROR_CODE);
    assert_eq!(
        mismatch.error.message,
        "lineRange must match the current anchored region 65-70"
    );
    Ok(())
}
