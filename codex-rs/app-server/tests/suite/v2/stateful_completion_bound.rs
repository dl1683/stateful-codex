use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

/// The scripted model repeats one invalid completion once past the three-rejection bound.
const SCRIPTED_ATTEMPTS: usize = 4;

/// A model that keeps sending the same invalid completion, as a provider bridge that
/// reshapes calls can, is stopped within the per-turn bound: three rejections, the third
/// telling it completion was not recorded, after which the turn's later requests no
/// longer offer `stateful_run_update` and a further attempt executes nothing. The turn
/// finishes with the model's answer, and the run stays open with the unrecorded
/// completion as a blocker instead of completing.
#[tokio::test]
async fn repeated_invalid_completions_stop_without_completing_the_run() -> Result<()> {
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
                name: "Completion retry bound".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "completion-bound-project".to_string(),
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
                goal: "Fix the calculator.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "completion-bound-run".to_string(),
            },
        })
        .await?;
    let invalid_completion = json!({
        "expectedRevision": started.run.revision,
        "status": "completed",
        "completionDisposition": "noReusableLearning",
        "result": "calc.add now adds.",
        "rootRevision": 0
    })
    .to_string();
    let mut bodies = (1..=SCRIPTED_ATTEMPTS)
        .map(|attempt| {
            responses::sse(vec![
                responses::ev_function_call(
                    &format!("complete-{attempt}"),
                    "stateful_run_update",
                    &invalid_completion,
                ),
                responses::ev_completed(&format!("complete-{attempt}-response")),
            ])
        })
        .collect::<Vec<_>>();
    bodies.push(responses::sse(vec![
        responses::ev_assistant_message(
            "answer",
            "calc.add now adds; the Stateful run was not completed.",
        ),
        responses::ev_completed("answer-response"),
    ]));
    let response_log = responses::mount_sse_sequence(&responses_server, bodies).await;

    let completed = server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Fix calc.add and finish.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    let offers_completion = requests
        .iter()
        .map(|request| {
            request.body_json()["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|tool| tool["name"] == "stateful_run_update")
        })
        .collect::<Vec<_>>();
    let last = requests.last().expect("the model was sampled");
    let stop = last
        .function_call_output_text("complete-3")
        .unwrap_or_default();
    let after_stop = last
        .function_call_output_text("complete-4")
        .unwrap_or_default();
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id.clone()),
                thread_id: None,
            },
        })
        .await?;
    let obligations: ObligationListResponse = server
        .request(|request_id| ClientRequest::ObligationList {
            request_id,
            params: ObligationListParams {
                run_id: started.run.id,
                cursor: None,
                limit: Some(10),
            },
        })
        .await?;
    let blockers = obligations
        .data
        .iter()
        .flat_map(|obligation| obligation.packet.blockers.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        (
            offers_completion,
            completed.turn.status,
            stop.contains("Stateful completion was NOT recorded")
                && stop.contains("do not call stateful_run_update again"),
            after_stop,
            read.run.map(|run| run.status),
            blockers.len(),
            blockers
                .first()
                .is_some_and(|blocker| blocker.contains("3 completion attempts were rejected")),
        ),
        (
            vec![true, true, true, false, false],
            TurnStatus::Completed,
            true,
            "unsupported call: stateful_run_update".to_string(),
            Some(StatefulRunStatus::Running),
            1,
            true,
        )
    );
    Ok(())
}
