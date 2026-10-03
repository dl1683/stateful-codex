//! Specialized Stateful tools are deferred to tool search for search-capable models: they
//! leave the initial roster, load through search, and run, including during a pending
//! Socratic run whose policy permits them.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::write_models_cache_with_models;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::load_default_config_for_test;
use core_test_support::responses;
use core_test_support::responses::ResponsesRequest;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

struct SearchSession {
    _codex_home: TempDir,
    server: TestAppServer,
    responses_server: wiremock::MockServer,
    thread_id: String,
    run_id: String,
}

/// A project thread with a run in `mode`, served by a model that supports tool search.
async fn search_session(mode: StatefulWorkflowMode) -> Result<SearchSession> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let config = load_default_config_for_test(&codex_home).await;
    let mut model_info =
        codex_core::test_support::construct_model_info_offline("mock-model", &config);
    model_info.supports_search_tool = true;
    write_models_cache_with_models(codex_home.path(), vec![model_info]).await?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Deferred tools".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "deferred-tools-project".to_string(),
            },
        })
        .await?;
    let thread_id = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread_id.clone(),
                goal: "Record what changed.".to_string(),
                mode,
                budget: StatefulRunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "deferred-tools-run".to_string(),
            },
        })
        .await?;
    Ok(SearchSession {
        _codex_home: codex_home,
        server,
        responses_server,
        thread_id,
        run_id: started.run.id,
    })
}

/// Searches for obligation_update, then calls it.
fn search_then_update() -> Vec<String> {
    vec![
        responses::sse(vec![
            responses::ev_response_created("search-response"),
            responses::ev_tool_search_call(
                "find-obligation",
                &json!({"query": "obligation_update semantic update", "limit": 4}),
            ),
            responses::ev_completed("search-response"),
        ]),
        responses::sse(vec![
            responses::ev_function_call(
                "loaded-obligation",
                "obligation_update",
                &json!({
                    "idempotencyKey": "loaded-obligation",
                    "packet": {
                        "learning": ["The loaded tool recorded this learning."],
                        "next": ["Confirm the next step with the user."]
                    }
                })
                .to_string(),
            ),
            responses::ev_completed("update-response"),
        ]),
        responses::sse(vec![
            responses::ev_assistant_message("done-message", "Recorded."),
            responses::ev_completed("done-response"),
        ]),
    ]
}

async fn run_turn(session: &mut SearchSession) -> Result<()> {
    session
        .server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: session.thread_id.clone(),
            input: vec![UserInput::Text {
                text: "Record what you learned.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

fn top_level_names(request: &ResponsesRequest) -> Vec<String> {
    request.body_json()["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

fn searched_names(request: &ResponsesRequest, call_id: &str) -> Vec<String> {
    let output = request.tool_search_output(call_id);
    output["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|tool| {
            let nested = tool["tools"].as_array().cloned().unwrap_or_default();
            std::iter::once(tool.clone()).chain(nested)
        })
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

async fn obligations(session: &mut SearchSession) -> Result<usize> {
    let listed: ObligationListResponse = session
        .server
        .request(|request_id| ClientRequest::ObligationList {
            request_id,
            params: ObligationListParams {
                run_id: session.run_id.clone(),
                cursor: None,
                limit: Some(10),
            },
        })
        .await?;
    Ok(listed.data.len())
}

#[tokio::test]
async fn deferred_stateful_tools_load_through_tool_search() -> Result<()> {
    let mut session = search_session(StatefulWorkflowMode::Collaborative).await?;
    let log = responses::mount_sse_sequence(&session.responses_server, search_then_update()).await;
    run_turn(&mut session).await?;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    let initial = top_level_names(&requests[0]);
    assert!(
        requests[0].body_json()["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|tool| tool["type"] == "tool_search"),
        "tool search should be offered: {initial:?}"
    );
    for deferred in [
        "obligation_update",
        "blackboard_update_batch",
        "steering_reconcile",
        "blackboard_query",
        "conversation_read",
        "blackboard_record_batch",
        "stateful_run_update",
        "evidence_read",
        "context_map_query",
    ] {
        assert!(
            !initial.iter().any(|name| name == deferred),
            "{deferred} in {initial:?}"
        );
    }
    for direct in ["memory_read"] {
        assert!(
            initial.iter().any(|name| name == direct),
            "{direct} missing from {initial:?}"
        );
    }
    assert!(
        searched_names(&requests[1], "find-obligation").contains(&"obligation_update".to_string())
    );
    let update_output = requests[2]
        .function_call_output_text("loaded-obligation")
        .expect("obligation output");
    assert!(update_output.contains("recorded"), "{update_output}");
    assert_eq!(obligations(&mut session).await?, 1);
    Ok(())
}

#[tokio::test]
async fn pending_socratic_run_can_discover_its_permitted_tools() -> Result<()> {
    let mut session = search_session(StatefulWorkflowMode::Socratic).await?;
    let log = responses::mount_sse_sequence(&session.responses_server, search_then_update()).await;
    run_turn(&mut session).await?;

    let requests = log.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        searched_names(&requests[1], "find-obligation").contains(&"obligation_update".to_string())
    );
    assert_eq!(obligations(&mut session).await?, 1);
    Ok(())
}
