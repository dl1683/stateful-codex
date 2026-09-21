use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server::INVALID_PARAMS_ERROR_CODE;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapCoverage as ApiContextMapCoverage;
use codex_app_server_protocol::ContextMapFreshness as ApiContextMapFreshness;
use codex_app_server_protocol::ContextMapQueryHit as ApiContextMapQueryHit;
use codex_app_server_protocol::ContextMapQueryParams;
use codex_app_server_protocol::ContextMapQueryResponse;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ContextMapSource as ApiContextMapSource;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_features::Feature;
use codex_project_intelligence::ContextMapCoverage;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchySourceUpdate;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewContextMapEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::NodeLifecycle;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::SourceFingerprint;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

#[tokio::test]
async fn context_map_query_is_explicitly_unavailable_without_sqlite() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri()).write(codex_home.path())?;
    let store_id = Uuid::now_v7();
    std::fs::write(
        codex_home.path().join("config.toml"),
        format!("experimental_thread_store = {{ type = \"in_memory\", id = \"{store_id}\" }}"),
    )?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;

    let request_id = server
        .send_request(
            "contextMap/query",
            Some(json!({
                "projectId": "project-1",
                "text": "operator setup",
            })),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request_id))
        .await?;
    assert_eq!(error.error.code, -32601);
    assert_eq!(
        error.error.message,
        "contextMap/query is unavailable without sqlite state"
    );
    Ok(())
}

#[tokio::test]
async fn context_map_query_returns_exact_routes_and_reports_stale_sources() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
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
                name: "Decisive Evidence Project".to_string(),
                roots: vec![ProjectRoot {
                    path: project_root_path.clone(),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "context-map-project".to_string(),
            },
        })
        .await?;

    let sqlite = SqliteConfig::new_for_testing(codex_home_path);
    let hierarchy = HierarchyStore::open(&sqlite).await?;
    let context_map = ContextMapStore::open(&sqlite).await?;
    let project_node_id = HierarchyNodeId::parse("project-node")?;
    let root_node_id = HierarchyNodeId::parse("root-node")?;
    let file_node_id = HierarchyNodeId::parse("readme-node")?;
    let source_fingerprint = SourceFingerprint::parse("sha256:readme-v1")?;
    let project_root_string = project_root_path.as_path().display().to_string();
    hierarchy
        .create_node(
            project_node_id.clone(),
            NewHierarchyNode {
                project_id: created.project.id.clone(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    hierarchy
        .create_node(
            root_node_id.clone(),
            NewHierarchyNode {
                project_id: created.project.id.clone(),
                parent_id: Some(project_node_id),
                kind: NodeKind::Directory,
                project_root: Some(project_root_string.clone()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    let file = hierarchy
        .create_node(
            file_node_id.clone(),
            NewHierarchyNode {
                project_id: created.project.id.clone(),
                parent_id: Some(root_node_id),
                kind: NodeKind::File,
                project_root: Some(project_root_string),
                relative_path: ProjectRelativePath::parse("README.md")?,
                region_anchor: None,
                source_fingerprint: Some(source_fingerprint.clone()),
            },
        )
        .await?;
    let entry = context_map
        .create_entry(
            ContextMapEntryId::parse("readme-map")?,
            NewContextMapEntry {
                project_id: created.project.id.clone(),
                node_id: file_node_id,
                source_fingerprint,
                description: "Project purpose, setup, and operator instructions.".to_string(),
                routing_terms: vec!["purpose".to_string(), "setup".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await?;

    let params = ContextMapQueryParams {
        project_id: created.project.id.clone(),
        text: "operator setup".to_string(),
        limit: Some(5),
    };
    let current: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery {
            request_id,
            params: params.clone(),
        })
        .await?;
    let expected_hit = ApiContextMapQueryHit {
        entry_id: entry.id.to_string(),
        node_id: entry.value.node_id.to_string(),
        source_fingerprint: entry.value.source_fingerprint.to_string(),
        description: entry.value.description.clone(),
        routing_terms: entry.value.routing_terms.clone(),
        coverage: ApiContextMapCoverage::Complete,
        freshness: ApiContextMapFreshness::Current,
        source: ApiContextMapSource {
            project_root: project_root_path,
            relative_path: "README.md".to_string(),
            region_anchor: None,
        },
        revision: entry.revision,
        created_at: entry.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: entry.updated_at_ms.div_euclid(/*rhs*/ 1000),
        last_verified_at: None,
    };
    assert_eq!(
        current,
        ContextMapQueryResponse {
            data: vec![expected_hit.clone()],
        }
    );

    hierarchy
        .update_source_state(
            &created.project.id,
            &file.id,
            HierarchySourceUpdate {
                expected_revision: file.revision,
                lifecycle: NodeLifecycle::Replaced,
                source_fingerprint: Some(SourceFingerprint::parse("sha256:readme-v2")?),
            },
        )
        .await?;
    let stale: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery { request_id, params })
        .await?;
    assert_eq!(
        stale,
        ContextMapQueryResponse {
            data: vec![ApiContextMapQueryHit {
                freshness: ApiContextMapFreshness::Stale,
                ..expected_hit
            }],
        }
    );

    let request_id = server
        .send_request(
            "contextMap/query",
            Some(json!({
                "projectId": "missing-project",
                "text": "operator setup",
                "limit": 5,
            })),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(RequestId::Integer(request_id))
        .await?;
    assert_eq!(error.error.code, INVALID_PARAMS_ERROR_CODE);
    assert_eq!(error.error.message, "project not found: missing-project");
    Ok(())
}

#[tokio::test]
async fn context_map_refresh_indexes_changes_and_marks_missing_sources() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("README.md"),
        "# Decisive operator setup\nUse the selected project root.\n",
    )?;
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
                name: "Indexed Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "context-map-refresh-project".to_string(),
            },
        })
        .await?;
    let params = ContextMapRefreshParams {
        project_id: created.project.id.clone(),
    };
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: params.clone(),
        })
        .await?;
    assert_eq!(
        refreshed,
        ContextMapRefreshResponse {
            files_indexed: 1,
            files_skipped: 0,
            missing_files: 0,
            truncated: false,
        }
    );
    let query = ContextMapQueryParams {
        project_id: created.project.id.clone(),
        text: "decisive operator".to_string(),
        limit: Some(5),
    };
    let current: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery {
            request_id,
            params: query.clone(),
        })
        .await?;
    assert_eq!(current.data.len(), 1);
    assert_eq!(current.data[0].freshness, ApiContextMapFreshness::Current);
    assert_eq!(current.data[0].source.relative_path, "README.md");

    std::fs::remove_file(project_root.path().join("README.md"))?;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh { request_id, params })
        .await?;
    assert_eq!(refreshed.missing_files, 1);
    let missing: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery {
            request_id,
            params: query,
        })
        .await?;
    assert_eq!(missing.data.len(), 1);
    assert_eq!(
        missing.data[0].freshness,
        ApiContextMapFreshness::SourceUnavailable
    );
    Ok(())
}
