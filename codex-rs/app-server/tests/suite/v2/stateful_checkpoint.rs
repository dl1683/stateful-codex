use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
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
use core_test_support::responses;
use serde_json::json;
use tempfile::TempDir;

/// Long stretches of tool work without a semantic update raise exactly one bounded
/// checkpoint nudge in the next model request.
#[tokio::test]
async fn tool_work_without_obligation_raises_one_checkpoint_nudge() -> Result<()> {
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
                name: "Checkpoint cadence".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "checkpoint-cadence-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let _started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: "Survey the project state.".to_string(),
                mode: StatefulWorkflowMode::Autonomous,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "checkpoint-cadence-run".to_string(),
            },
        })
        .await?;
    let mut first = vec![responses::ev_response_created("tool-work")];
    for index in 0..8 {
        first.push(responses::ev_function_call(
            &format!("query-{index}"),
            "blackboard_query",
            &json!({"text": format!("topic {index}")}).to_string(),
        ));
    }
    first.push(responses::ev_completed("tool-work"));
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(first),
            responses::sse(vec![
                responses::ev_assistant_message("surveyed", "Surveyed."),
                responses::ev_completed("surveyed-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Survey what the project knows.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert!(requests[0].body_contains_text("Semantic checkpoint: none due"));
    assert!(!requests[0].body_contains_text("advisory due"));
    assert!(
        requests[1]
            .body_contains_text("Semantic checkpoint: advisory due (process-local checkpoint 1)")
    );
    Ok(())
}

#[tokio::test]
async fn unanswered_checkpoint_escalates_and_an_obligation_update_resets_it() -> Result<()> {
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
                name: "Checkpoint escalation".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "checkpoint-escalation-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let _started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: "Survey the project state.".to_string(),
                mode: StatefulWorkflowMode::Autonomous,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "checkpoint-escalation-run".to_string(),
            },
        })
        .await?;
    let queries = |batch: usize| {
        let mut events = vec![responses::ev_response_created(&format!("batch-{batch}"))];
        for index in 0..8 {
            events.push(responses::ev_function_call(
                &format!("query-{batch}-{index}"),
                "blackboard_query",
                &json!({"text": format!("topic {batch} {index}")}).to_string(),
            ));
        }
        events.push(responses::ev_completed(&format!("batch-{batch}")));
        responses::sse(events)
    };
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            queries(1),
            queries(2),
            responses::sse(vec![
                responses::ev_function_call(
                    "checkpoint-update",
                    "obligation_update",
                    &json!({
                        "idempotencyKey": "checkpoint-update",
                        "packet": {
                            "learning": ["The project has no promoted knowledge yet."],
                            "next": ["Refresh the context map before recording findings."]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("checkpoint-update-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("surveyed", "Surveyed."),
                responses::ev_completed("surveyed-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Survey what the project knows.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 4);
    assert!(
        requests[1]
            .body_contains_text("Semantic checkpoint: advisory due (process-local checkpoint 1)")
    );
    assert!(
        requests[2].body_contains_text("Semantic checkpoint: overdue (process-local checkpoint 2)")
    );
    assert!(requests[2].body_contains_text("Call obligation_update now"));
    let latest_checkpoint = |body: String| {
        body.rfind("Semantic checkpoint: ")
            .map(|at| body[at..].chars().take(40).collect::<String>())
    };
    assert_eq!(
        latest_checkpoint(requests[3].body_json().to_string()).as_deref(),
        Some("Semantic checkpoint: none due from succe")
    );
    Ok(())
}
