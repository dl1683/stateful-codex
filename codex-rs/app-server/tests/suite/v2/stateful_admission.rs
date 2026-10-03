//! Durable run admission through the public API: a thread's turns stay attributed to the run
//! they are bound to, even when another active run of the thread was updated more recently.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulMeasurementListParams;
use codex_app_server_protocol::StatefulMeasurementListResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
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

#[tokio::test]
async fn turns_stay_with_their_bound_run_when_a_newer_run_appears() -> Result<()> {
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
                name: "Admission".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "admission-project".to_string(),
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
                goal: "Keep working on the scaler.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 600,
                },
                idempotency_key: "admission-run".to_string(),
            },
        })
        .await?;
    run_turn(&mut server, &thread.thread.id).await?;

    // Another active run of the same thread, updated after the first turn.
    StatefulRunStore::open(&SqliteConfig::new_for_testing(codex_home.path().abs()))
        .await?
        .create_run(
            StatefulRunId::parse("stateful-run-newer")?,
            NewStatefulRun {
                project_id: project.project.id.clone(),
                thread_ids: vec![thread.thread.id.clone()],
                goal: "An unrelated newer run.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await?;
    run_turn(&mut server, &thread.thread.id).await?;

    let measurements: StatefulMeasurementListResponse = server
        .request(|request_id| ClientRequest::StatefulMeasurementList {
            request_id,
            params: StatefulMeasurementListParams {
                project_id: project.project.id.clone(),
                cursor: None,
                limit: Some(5),
            },
        })
        .await?;
    assert_eq!(
        measurements
            .data
            .iter()
            .map(|measurement| measurement.run_id.clone())
            .collect::<Vec<_>>(),
        vec![started.run.id.clone(), started.run.id]
    );
    Ok(())
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: "Continue.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}
