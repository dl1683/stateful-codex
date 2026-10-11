//! Ask: a project-attached thread that never had a run gets the project's memory but no
//! run-bound tool and no run guidance, so nothing it does can move a run's lifecycle. A thread
//! with a run keeps every tool (also after the run ended, so it can still read that run; the
//! paged-result tests in stateful_run.rs cover that).

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::responses::ResponsesRequest;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const RUN_TOOLS: [&str; 6] = [
    "obligation_update",
    "stateful_run_update",
    "stateful_acceptance_update",
    "stateful_run_read",
    "steering_query",
    "steering_reconcile",
];

fn tool_names(request: &ResponsesRequest) -> Vec<String> {
    request.body_json()["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

/// For each request: which run-bound tools it offers, whether it offers the project memory
/// tool, and whether it carries the project and run world-state sections.
fn roster(request: &ResponsesRequest) -> (Vec<String>, bool, bool, bool) {
    let names = tool_names(request);
    (
        RUN_TOOLS
            .iter()
            .filter(|tool| names.iter().any(|name| name == *tool))
            .map(ToString::to_string)
            .collect(),
        names.iter().any(|name| name == "memory_read"),
        request.body_contains_text("<stateful_project>"),
        request.body_contains_text("<stateful_run>"),
    )
}

async fn turn(server: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn an_ask_thread_has_project_memory_but_no_run_tools_or_run_guidance() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Ask".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "ask-project".to_string(),
            },
        })
        .await?;
    let project_id = project.project.id;
    let answer = |id: &str, text: &str| {
        responses::sse(vec![
            responses::ev_response_created(id),
            responses::ev_assistant_message(&format!("{id}-message"), text),
            responses::ev_completed(id),
        ])
    };
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![answer("ask", "Paris."), answer("work", "Working on it.")],
    )
    .await;

    let ask = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    turn(&mut server, &ask, "What is the capital of France?").await?;
    let ask_run: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(ask.clone()),
            },
        })
        .await?;

    let work = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let _: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id,
                thread_id: work.clone(),
                goal: "Refactor the parser.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "ask-adjacent-work".to_string(),
            },
        })
        .await?;
    turn(&mut server, &work, "Refactor the parser.").await?;

    let requests = log.requests();
    assert_eq!(
        (ask_run.run, requests.iter().map(roster).collect::<Vec<_>>()),
        (
            None,
            vec![
                (Vec::new(), true, true, false),
                (
                    RUN_TOOLS.iter().map(ToString::to_string).collect(),
                    true,
                    true,
                    true
                ),
            ]
        )
    );
    Ok(())
}
