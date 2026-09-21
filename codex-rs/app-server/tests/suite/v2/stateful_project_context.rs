use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathExt;
use tempfile::TempDir;

#[tokio::test]
async fn selected_project_context_survives_fork_and_cold_resume() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
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
                name: "Decisive Evidence Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-context-project".to_string(),
            },
        })
        .await?;
    seed_root_blackboard(codex_home.path(), &created.project.id).await?;
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;
    run_turn(&mut server, &started.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    let forked: ThreadForkResponse = server
        .request(|request_id| ClientRequest::ThreadFork {
            request_id,
            params: ThreadForkParams {
                thread_id: started.thread.id.clone(),
                ..Default::default()
            },
        })
        .await?;
    run_turn(&mut server, &forked.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    drop(server);
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let resumed: ThreadResumeResponse = server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: started.thread.id.clone(),
                ..Default::default()
            },
        })
        .await?;
    run_turn(&mut server, &resumed.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    let unselected = server.start_thread(ThreadStartParams::default()).await?;
    run_turn(&mut server, &unselected.thread.id).await?;
    let requests = responses.received_requests().await.unwrap_or_default();
    let body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("model request should be recorded")
        .body_json::<serde_json::Value>()?
        .to_string();
    assert!(!body.contains("<stateful_project>"));
    Ok(())
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: "Continue the project work.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

async fn assert_latest_request_has_project(
    responses: &wiremock::MockServer,
    project_id: &str,
) -> Result<()> {
    let requests = responses.received_requests().await.unwrap_or_default();
    let body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("model request should be recorded")
        .body_json::<serde_json::Value>()?;
    let body = body.to_string();
    assert!(body.contains("<stateful_project>"));
    assert!(body.contains(&format!("Project ID: {project_id}")));
    assert!(body.contains("Decisive Evidence Project"));
    assert!(body.contains("A decisive project fact survives every thread view."));
    assert!(body.contains("verification=unverified"));
    Ok(())
}

async fn seed_root_blackboard(codex_home: &std::path::Path, project_id: &str) -> Result<()> {
    let sqlite = SqliteConfig::new_for_testing(codex_home.abs());
    let hierarchy = HierarchyStore::open(&sqlite).await?;
    let project_node_id = HierarchyNodeId::parse(format!("project-node-{project_id}"))?;
    hierarchy
        .create_node(
            project_node_id.clone(),
            NewHierarchyNode {
                project_id: project_id.to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    BlackboardStore::open(&sqlite)
        .await?
        .create_entry(
            BlackboardEntryId::parse(format!("project-fact-{project_id}"))?,
            NewBlackboardEntry {
                project_id: project_id.to_string(),
                node_id: project_node_id,
                kind: BlackboardKind::Fact,
                content: "A decisive project fact survives every thread view.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(8_500)?,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "integration-fixture".to_string(),
                },
            },
        )
        .await?;
    Ok(())
}
