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
use codex_utils_absolute_path::AbsolutePathBuf;
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
    Ok(())
}
