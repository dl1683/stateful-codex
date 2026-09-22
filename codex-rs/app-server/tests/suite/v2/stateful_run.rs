use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ObligationUpdatedNotification;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunPauseParams;
use codex_app_server_protocol::StatefulRunPauseResponse;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunResumeParams;
use codex_app_server_protocol::StatefulRunResumeResponse;
use codex_app_server_protocol::StatefulRunSetModeParams;
use codex_app_server_protocol::StatefulRunSetModeResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulRunUpdatedNotification;
use codex_app_server_protocol::StatefulSteeringStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::SteeringListParams;
use codex_app_server_protocol::SteeringListResponse;
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::SteeringUpdatedNotification;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn stateful_run_preserves_explicit_mode_and_reconciles_live_controls() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
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
                name: "Long-running investigation".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "stateful-run-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;

    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id.clone(),
                thread_id: thread.thread.id.clone(),
                goal: "Find the decisive constraint and produce a verified result.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "first-run".to_string(),
            },
        })
        .await?;
    assert_eq!(started.run.mode, StatefulWorkflowMode::Collaborative);
    assert_eq!(started.run.continuations_used, 0);
    assert_eq!(started.run.status, StatefulRunStatus::Running);
    let started_notification: StatefulRunUpdatedNotification =
        server.read_notification("statefulRun/updated").await?;
    assert_eq!(started_notification.run_id, started.run.id);
    assert_eq!(started_notification.revision, started.run.revision);

    let steering: SteeringSubmitResponse = server
        .request(|request_id| ClientRequest::SteeringSubmit {
            request_id,
            params: SteeringSubmitParams {
                run_id: started.run.id.clone(),
                input: "Connect the deployment finding to the source constraint.".to_string(),
                affected_obligation_ids: Vec::new(),
                idempotency_key: "deployment-connection".to_string(),
            },
        })
        .await?;
    let steering_notification: SteeringUpdatedNotification =
        server.read_notification("steering/updated").await?;
    assert_eq!(steering_notification.steering_id, steering.steering.id);
    let steering_page: SteeringListResponse = server
        .request(|request_id| ClientRequest::SteeringList {
            request_id,
            params: SteeringListParams {
                run_id: started.run.id.clone(),
                cursor: None,
                limit: Some(10),
            },
        })
        .await?;
    assert_eq!(steering_page.data, vec![steering.steering]);

    let socratic: StatefulRunSetModeResponse = server
        .request(|request_id| ClientRequest::StatefulRunSetMode {
            request_id,
            params: StatefulRunSetModeParams {
                run_id: started.run.id.clone(),
                expected_revision: started.run.revision,
                mode: StatefulWorkflowMode::Socratic,
            },
        })
        .await?;
    assert_eq!(socratic.run.mode, StatefulWorkflowMode::Socratic);
    assert_eq!(socratic.run.status, StatefulRunStatus::Pending);
    let collaborative: StatefulRunSetModeResponse = server
        .request(|request_id| ClientRequest::StatefulRunSetMode {
            request_id,
            params: StatefulRunSetModeParams {
                run_id: socratic.run.id,
                expected_revision: socratic.run.revision,
                mode: StatefulWorkflowMode::Collaborative,
            },
        })
        .await?;
    assert_eq!(collaborative.run.mode, StatefulWorkflowMode::Collaborative);
    assert_eq!(collaborative.run.status, StatefulRunStatus::Running);

    let paused: StatefulRunPauseResponse = server
        .request(|request_id| ClientRequest::StatefulRunPause {
            request_id,
            params: StatefulRunPauseParams {
                run_id: started.run.id.clone(),
                expected_revision: collaborative.run.revision,
            },
        })
        .await?;
    assert_eq!(paused.run.status, StatefulRunStatus::Paused);
    let resumed: StatefulRunResumeResponse = server
        .request(|request_id| ClientRequest::StatefulRunResume {
            request_id,
            params: StatefulRunResumeParams {
                run_id: paused.run.id.clone(),
                expected_revision: paused.run.revision,
            },
        })
        .await?;
    assert_eq!(resumed.run.status, StatefulRunStatus::Running);
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(thread.thread.id),
            },
        })
        .await?;
    assert_eq!(read.run, Some(resumed.run));
    Ok(())
}

#[tokio::test]
async fn model_updates_semantic_progress_and_applies_user_steering() -> Result<()> {
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
                name: "Steerable investigation".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "steerable-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: "Find and verify the decisive source connection.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "model-run".to_string(),
            },
        })
        .await?;
    let submitted: SteeringSubmitResponse = server
        .request(|request_id| ClientRequest::SteeringSubmit {
            request_id,
            params: SteeringSubmitParams {
                run_id: started.run.id.clone(),
                input: "Connect the source constraint to deployment risk.".to_string(),
                affected_obligation_ids: Vec::new(),
                idempotency_key: "connect-risk".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "update-obligation",
                    "obligation_update",
                    &json!({
                        "idempotencyKey": "decisive-connection",
                        "packet": {
                            "examined": ["The source constraint and deployment finding."],
                            "rationale": ["Their interaction controls the recommended design."],
                            "learning": ["The deployment risk is triggered by the source constraint."],
                            "implication": ["The strategy must verify that connection before implementation."],
                            "next": ["Verify both exact source regions."]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("update-obligation-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "acknowledge-steering",
                    "steering_reconcile",
                    &json!({
                        "steeringId": submitted.steering.id.clone(),
                        "expectedRevision": 1,
                        "action": "acknowledge"
                    })
                    .to_string(),
                ),
                responses::ev_completed("acknowledge-steering-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "apply-steering",
                    "steering_reconcile",
                    &json!({
                        "steeringId": submitted.steering.id.clone(),
                        "expectedRevision": 2,
                        "action": "apply",
                        "expectedRunRevision": 1,
                        "strategy": "Verify the constraint-to-deployment-risk connection first."
                    })
                    .to_string(),
                ),
                responses::ev_completed("apply-steering-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "complete-run",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": 2,
                        "status": "completed",
                        "result": "Verified the decisive connection and incorporated the user's direction."
                    })
                    .to_string(),
                ),
                responses::ev_completed("complete-run-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Continue and incorporate my steering.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let obligation_event: ObligationUpdatedNotification =
        server.read_notification("obligation/updated").await?;
    assert_eq!(obligation_event.run_id, started.run.id);
    assert_eq!(obligation_event.revision, 1);
    let mut last_steering_event = None;
    for _ in 0..3 {
        last_steering_event = Some(
            server
                .read_notification::<SteeringUpdatedNotification>("steering/updated")
                .await?,
        );
    }
    assert_eq!(
        last_steering_event
            .expect("applied steering event")
            .revision,
        3
    );
    let mut last_run_event = None;
    for _ in 0..3 {
        last_run_event = Some(
            server
                .read_notification::<StatefulRunUpdatedNotification>("statefulRun/updated")
                .await?,
        );
    }
    assert_eq!(last_run_event.expect("completed run event").revision, 3);

    let requests = response_log.requests();
    assert_eq!(requests.len(), 5);
    assert!(requests[0].body_contains_text("<stateful_run>"));
    assert!(requests[0].body_contains_text("Connect the source constraint to deployment risk."));
    assert!(requests[0].body_contains_text(&submitted.steering.id));
    let obligations: ObligationListResponse = server
        .request(|request_id| ClientRequest::ObligationList {
            request_id,
            params: ObligationListParams {
                run_id: started.run.id.clone(),
                cursor: None,
                limit: Some(10),
            },
        })
        .await?;
    assert_eq!(obligations.data.len(), 1);
    assert_eq!(
        obligations.data[0].packet.learning,
        vec!["The deployment risk is triggered by the source constraint."]
    );
    let steering: SteeringListResponse = server
        .request(|request_id| ClientRequest::SteeringList {
            request_id,
            params: SteeringListParams {
                run_id: started.run.id.clone(),
                cursor: None,
                limit: Some(10),
            },
        })
        .await?;
    assert_eq!(steering.data[0].status, StatefulSteeringStatus::Applied);
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id),
                thread_id: None,
            },
        })
        .await?;
    assert_eq!(
        read.run.expect("completed run remains readable").status,
        StatefulRunStatus::Completed
    );
    Ok(())
}
