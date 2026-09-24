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
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn model_cannot_complete_a_run_with_unresolved_user_steering() -> Result<()> {
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
                name: "Steering completion guard".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "steering-completion-project".to_string(),
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
                goal: "Respect user direction before completion.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "steering-completion-run".to_string(),
            },
        })
        .await?;
    let submitted: SteeringSubmitResponse = server
        .request(|request_id| ClientRequest::SteeringSubmit {
            request_id,
            params: SteeringSubmitParams {
                run_id: started.run.id.clone(),
                input: "Verify the decisive source before finishing.".to_string(),
                affected_obligation_ids: Vec::new(),
                idempotency_key: "verify-before-finishing".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "premature-completion",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "result": "The work is complete.",
                        "rootRevision": 0,
                        "materialRootFindings": [],
                        "completionIdempotencyKey": "premature-final",
                        "finalObligation": {
                            "learning": ["The work produced a final result."],
                            "implication": ["The result would otherwise be ready to use."]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("premature-completion-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "completion-rejected",
                    "I must reconcile the unresolved steering before completion.",
                ),
                responses::ev_completed("completion-rejected-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Finish the work.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    let rejection = requests[1].function_call_output("premature-completion");
    assert!(rejection.to_string().contains(&submitted.steering.id));
    assert!(rejection.to_string().contains("is submitted"));
    assert!(rejection.to_string().contains("apply or reject"));
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id.clone()),
                thread_id: None,
            },
        })
        .await?;
    assert_eq!(
        read.run.expect("run remains readable").status,
        StatefulRunStatus::Running
    );
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
    assert_eq!(obligations.data, Vec::new());
    Ok(())
}
