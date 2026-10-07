//! `statefulRun/cancel` interrupts the run's in-flight turn instead of only
//! recording a status, and no Autonomous continuation follows it.

use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunCancelParams;
use codex_app_server_protocol::StatefulRunCancelResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

async fn wait_for_model_requests(server: &MockServer, expected: usize) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        while server.received_requests().await.unwrap_or_default().len() < expected {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn cancel_interrupts_the_in_flight_turn_and_fences_continuation() -> Result<()> {
    let responses = MockServer::start().await;
    // The model never answers in time, so the turn stays in flight until cancelled.
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("")
                .set_delay(Duration::from_secs(600)),
        )
        .mount(&responses)
        .await;
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
                name: "Cancellable work".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "cancel-project".to_string(),
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
                goal: "Run the long command.".to_string(),
                mode: StatefulWorkflowMode::Autonomous,
                budget: StatefulRunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "cancel-run".to_string(),
            },
        })
        .await?;
    let turn: TurnStartResponse = server
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.thread.id.clone(),
                input: vec![UserInput::Text {
                    text: "Run the long command.".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    wait_for_model_requests(&responses, /*expected*/ 1).await?;

    let cancelled: StatefulRunCancelResponse = server
        .request(|request_id| ClientRequest::StatefulRunCancel {
            request_id,
            params: StatefulRunCancelParams {
                run_id: started.run.id.clone(),
                expected_revision: started.run.revision,
            },
        })
        .await?;
    let cancelled_revision = cancelled.run.revision;
    assert_eq!(
        (cancelled.run.status, cancelled.interrupted_turn_ids),
        (StatefulRunStatus::Cancelled, vec![turn.turn.id.clone()])
    );
    let completed: TurnCompletedNotification = server.read_notification("turn/completed").await?;
    assert_eq!(
        (completed.turn.id, completed.turn.status),
        (turn.turn.id, TurnStatus::Interrupted)
    );

    // No continuation turn may reach the model after the cancel.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        responses
            .received_requests()
            .await
            .unwrap_or_default()
            .len(),
        1
    );

    // A later run's in-flight turn on the same thread is not the cancelled run's turn.
    let later: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: started.run.project_id.clone(),
                thread_id: thread.thread.id.clone(),
                goal: "Continue differently.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "later-run".to_string(),
            },
        })
        .await?;
    let later_turn: TurnStartResponse = server
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.thread.id.clone(),
                input: vec![UserInput::Text {
                    text: "Continue differently.".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    wait_for_model_requests(&responses, /*expected*/ 2).await?;
    let recancelled: StatefulRunCancelResponse = server
        .request(|request_id| ClientRequest::StatefulRunCancel {
            request_id,
            params: StatefulRunCancelParams {
                run_id: started.run.id.clone(),
                expected_revision: cancelled_revision,
            },
        })
        .await?;
    assert_eq!(
        (recancelled.run.status, recancelled.interrupted_turn_ids),
        (StatefulRunStatus::Cancelled, Vec::<String>::new())
    );
    assert!(
        tokio::time::timeout(
            Duration::from_secs(1),
            server.read_notification::<TurnCompletedNotification>("turn/completed"),
        )
        .await
        .is_err(),
        "the later run's turn {} must keep running",
        later_turn.turn.id
    );
    assert_eq!(later.run.status, StatefulRunStatus::Running);
    Ok(())
}

/// Another process can create a second live run on the same thread; cancelling it
/// must not interrupt the turn bound to the first run.
#[tokio::test]
async fn cancel_never_interrupts_a_turn_bound_to_another_run() -> Result<()> {
    let responses = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string("")
                .set_delay(Duration::from_secs(600)),
        )
        .mount(&responses)
        .await;
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
                name: "Shared thread".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "shared-thread-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let _: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id.clone(),
                thread_id: thread.thread.id.clone(),
                goal: "Own the in-flight turn.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "owning-run".to_string(),
            },
        })
        .await?;
    let turn: TurnStartResponse = server
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread.thread.id.clone(),
                input: vec![UserInput::Text {
                    text: "Keep working.".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            },
        })
        .await?;
    wait_for_model_requests(&responses, /*expected*/ 1).await?;

    // A second process registers its own live run on the same thread.
    let other = StatefulRunStore::open(&SqliteConfig::new_for_testing(codex_home.path().abs()))
        .await?
        .create_run(
            StatefulRunId::parse("run-from-another-process")?,
            NewStatefulRun {
                project_id: project.project.id,
                thread_ids: vec![thread.thread.id.clone()],
                goal: "A concurrent run.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await?;
    let cancelled: StatefulRunCancelResponse = server
        .request(|request_id| ClientRequest::StatefulRunCancel {
            request_id,
            params: StatefulRunCancelParams {
                run_id: other.id.to_string(),
                expected_revision: other.revision,
            },
        })
        .await?;
    assert_eq!(
        (cancelled.run.status, cancelled.interrupted_turn_ids),
        (StatefulRunStatus::Cancelled, Vec::<String>::new())
    );
    assert!(
        tokio::time::timeout(
            Duration::from_secs(1),
            server.read_notification::<TurnCompletedNotification>("turn/completed"),
        )
        .await
        .is_err(),
        "turn {} belongs to the first run and must keep running",
        turn.turn.id
    );
    Ok(())
}
