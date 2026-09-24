use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunResumeParams;
use codex_app_server_protocol::StatefulRunResumeResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulRunUpdatedNotification;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn autonomous_run_continues_after_idle_until_the_model_completes_it() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let (project_id, thread_id) = project_and_thread(&mut server, "autonomous").await?;
    let started = start_run(
        &mut server,
        project_id,
        thread_id.clone(),
        StatefulWorkflowMode::Autonomous,
        "autonomous-run",
    )
    .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_assistant_message(
                    "initial-progress",
                    "I found the first dependency; more work remains.",
                ),
                responses::ev_completed("initial-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "complete-autonomous-run",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": 2,
                        "status": "completed",
                        "result": "The unattended investigation reached its evidence-grounded result.",
                        "rootRevision": 0,
                        "materialRootFindings": [],
                        "completionIdempotencyKey": "autonomous-result",
                        "finalObligation": {
                            "learning": ["The unattended investigation reached its evidence-grounded result."],
                            "implication": ["The run can now complete without user intervention."]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("complete-autonomous-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("autonomous-done", "Investigation complete."),
                responses::ev_completed("autonomous-done-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "next-question-done",
                    "I continued from the durable prior outcome.",
                ),
                responses::ev_completed("next-question-done-response"),
            ]),
        ],
    )
    .await;

    start_turn(&mut server, thread_id.clone(), "Begin the investigation.").await?;
    let mut revisions = Vec::new();
    for _ in 0..3 {
        revisions.push(
            server
                .read_notification::<StatefulRunUpdatedNotification>("statefulRun/updated")
                .await?
                .revision,
        );
    }
    assert_eq!(revisions, vec![1, 2, 3]);
    let read = read_run(&mut server, started.run.id).await?;
    let completed = read.run.expect("run remains readable");
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    assert_eq!(completed.continuations_used, 1);
    assert!(
        read.recovery
            .expect("autonomous recovery state is visible")
            .previous_turn_id
            .is_some()
    );
    let _: TurnCompletedNotification = server.read_notification("turn/completed").await?;
    start_turn(
        &mut server,
        thread_id,
        "Continue with the next project question.",
    )
    .await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[1].body_contains_text("Continue Autonomous Stateful run"));
    assert!(requests[1].body_contains_text(
        "do not repeat completed work or reopen unchanged host-audited sourceVerified evidence"
    ));
    assert!(requests[3].body_contains_text("Recent completed project outcomes"));
    assert!(
        requests[3].body_contains_text(
            "The unattended investigation reached its evidence-grounded result."
        )
    );
    assert!(requests[3].body_contains_text("The run can now complete without user intervention."));
    Ok(())
}

#[tokio::test]
async fn socratic_run_blocks_execution_until_the_user_resumes_it() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let (project_id, thread_id) = project_and_thread(&mut server, "socratic").await?;
    let started = start_run(
        &mut server,
        project_id,
        thread_id.clone(),
        StatefulWorkflowMode::Socratic,
        "socratic-run",
    )
    .await?;
    assert_eq!(started.run.status, StatefulRunStatus::Pending);
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "blocked-exec",
                    "exec_command",
                    &json!({"cmd": "Write-Output SHOULD_NOT_RUN"}).to_string(),
                ),
                responses::ev_completed("blocked-exec-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "socratic-question",
                    "I will resolve the assumptions before execution.",
                ),
                responses::ev_completed("socratic-question-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "allowed-exec",
                    "exec_command",
                    &json!({"cmd": "Write-Output SOCRATIC_RESUMED"}).to_string(),
                ),
                responses::ev_completed("allowed-exec-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("execution-done", "Execution was allowed."),
                responses::ev_completed("execution-done-response"),
            ]),
        ],
    )
    .await;

    start_turn(
        &mut server,
        thread_id.clone(),
        "Explore this problem first.",
    )
    .await?;
    let first_requests = response_log.requests();
    assert_eq!(first_requests.len(), 2);
    assert!(
        first_requests[1]
            .function_call_output("blocked-exec")
            .to_string()
            .contains("blocked while Socratic run")
    );

    let resumed: StatefulRunResumeResponse = server
        .request(|request_id| ClientRequest::StatefulRunResume {
            request_id,
            params: StatefulRunResumeParams {
                run_id: started.run.id,
                expected_revision: started.run.revision,
            },
        })
        .await?;
    assert_eq!(resumed.run.status, StatefulRunStatus::Running);
    start_turn(&mut server, thread_id, "Execute the agreed next step.").await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 4);
    assert!(
        requests[3]
            .function_call_output("allowed-exec")
            .to_string()
            .contains("SOCRATIC_RESUMED")
    );
    Ok(())
}

async fn project_and_thread(server: &mut TestAppServer, key: &str) -> Result<(String, String)> {
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Stateful mode test".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: format!("{key}-project"),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    Ok((project.project.id, thread.thread.id))
}

async fn start_run(
    server: &mut TestAppServer,
    project_id: String,
    thread_id: String,
    mode: StatefulWorkflowMode,
    key: &str,
) -> Result<StatefulRunStartResponse> {
    server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id,
                thread_id,
                goal: "Reach the explicit goal using the selected workflow mode.".to_string(),
                mode,
                budget: StatefulRunBudget {
                    max_continuations: 2,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: key.to_string(),
            },
        })
        .await
}

async fn start_turn(server: &mut TestAppServer, thread_id: String, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id,
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

async fn read_run(server: &mut TestAppServer, run_id: String) -> Result<StatefulRunReadResponse> {
    let response: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id),
                thread_id: None,
            },
        })
        .await?;
    Ok(response)
}
