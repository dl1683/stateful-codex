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
    assert_eq!(status.hierarchy_node_count, 3);
    assert_eq!(status.file_count, 1);
    assert_eq!(status.missing_source_count, 0);
    assert_eq!(status.context_map_entry_count, 1);
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
        ]
    );

    let routes: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery {
            request_id,
            params: ContextMapQueryParams {
                project_id: status.project_id.clone(),
                text: "decisive evidence".to_string(),
                limit: Some(5),
            },
        })
        .await?;
    let route = routes
        .data
        .into_iter()
        .next()
        .expect("indexed source route");
    let evidence: EvidenceReadResponse = server
        .request(|request_id| ClientRequest::EvidenceRead {
            request_id,
            params: EvidenceReadParams {
                project_id: status.project_id.clone(),
                context_map_entry_id: route.entry_id.clone(),
                max_bytes: Some(12),
            },
        })
        .await?;
    assert_eq!(
        evidence,
        EvidenceReadResponse {
            project_id: status.project_id.clone(),
            context_map_entry_id: route.entry_id.clone(),
            node_id: route.node_id,
            source_fingerprint: route.source_fingerprint,
            source: route.source,
            encoding: EvidenceEncoding::Utf8,
            content: "decisive=42\n".to_string(),
            bytes_returned: 12,
            total_bytes: 16,
            truncated: true,
            revision: route.revision,
        }
    );

    std::fs::write(source_path, "decisive=99\nnext")?;
    let request_id = server
        .send_request(
            "evidence/read",
            Some(json!({
                "projectId": status.project_id,
                "contextMapEntryId": route.entry_id,
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
