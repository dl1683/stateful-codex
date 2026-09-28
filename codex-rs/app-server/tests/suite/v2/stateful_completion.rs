use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::BlackboardConfirmParams;
use codex_app_server_protocol::BlackboardConfirmResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
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
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
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
                            "implication": ["The result would otherwise be ready to use."],
                            "uncertainty": ["The submitted user direction remains unresolved."]
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

#[tokio::test]
async fn model_cannot_complete_with_reusable_learning_absent_from_the_blackboard() -> Result<()> {
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
                name: "Completion learning guard".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "completion-learning-project".to_string(),
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
                goal: "Preserve the reusable conclusion.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "completion-learning-run".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "uncaptured-learning",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "result": "The decisive threshold is six.",
                        "rootRevision": 0,
                        "materialRootFindings": [],
                        "completionIdempotencyKey": "uncaptured-learning-final",
                        "finalObligation": {
                            "learning": ["The decisive threshold is six."],
                            "implication": ["Later work must apply that threshold."]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("uncaptured-learning-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "learning-capture-required",
                    "I must record the reusable conclusion before completion.",
                ),
                responses::ev_completed("learning-capture-required-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Finish and retain what you learned.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    let rejection = requests[1].function_call_output("uncaptured-learning");
    assert!(
        rejection
            .to_string()
            .contains("completion selected no blackboard finding")
    );
    assert!(
        rejection
            .to_string()
            .contains("record and promote the smallest durable conclusion")
    );
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

#[tokio::test]
async fn model_completes_a_lookup_without_durable_learning_ceremony() -> Result<()> {
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
                name: "Proportional completion".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "proportional-completion-project".to_string(),
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
                goal: "Answer a lookup.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "proportional-completion-run".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "mixed-disposition",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "completionDisposition": "noReusableLearning",
                        "result": "The release gate is build C7-42.",
                        "finalObligation": {"learning": ["The release gate is build C7-42."]}
                    })
                    .to_string(),
                ),
                responses::ev_completed("mixed-disposition-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "lookup-complete",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "completionDisposition": "noReusableLearning",
                        "result": "The release gate is build C7-42."
                    })
                    .to_string(),
                ),
                responses::ev_completed("lookup-complete-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "lookup-answer",
                    "The release gate is build C7-42.",
                ),
                responses::ev_completed("lookup-answer-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "What is the release gate?".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[1]
            .function_call_output("mixed-disposition")
            .to_string()
            .contains("noReusableLearning completes with the result only")
    );
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id.clone()),
                thread_id: None,
            },
        })
        .await?;
    let run = read.run.expect("run remains readable");
    assert_eq!(
        (run.status, run.result),
        (
            StatefulRunStatus::Completed,
            Some("The release gate is build C7-42.".to_string())
        )
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

#[tokio::test]
async fn lookup_completion_is_refused_after_recording_project_knowledge() -> Result<()> {
    let (output, status) = complete_lookup_after_recording(ProjectShape::Indexed).await?;
    assert!(
        output.contains("agent-written project knowledge changed in this project during this run")
    );
    assert_eq!(status, StatefulRunStatus::Running);
    Ok(())
}

#[tokio::test]
async fn lookup_completion_is_allowed_when_every_record_failed() -> Result<()> {
    let (output, status) = complete_lookup_after_recording(ProjectShape::Unindexed).await?;
    assert!(output.contains(r#"\"status\":\"completed\""#), "{output}");
    assert_eq!(status, StatefulRunStatus::Completed);
    Ok(())
}

/// Whether the project has an indexed hierarchy, which blackboard records require.
enum ProjectShape {
    Indexed,
    Unindexed,
}

/// Records one finding, then attempts a `noReusableLearning` completion. Returns the
/// completion tool output and the run status afterwards.
async fn complete_lookup_after_recording(
    shape: ProjectShape,
) -> Result<(String, StatefulRunStatus)> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let roots = match shape {
        ProjectShape::Indexed => vec![ProjectRoot {
            path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                .expect("temporary project root should be absolute"),
        }],
        ProjectShape::Unindexed => Vec::new(),
    };
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Proportional completion guard".to_string(),
                roots,
                metadata: None,
                idempotency_key: "proportional-guard-project".to_string(),
            },
        })
        .await?;
    if let ProjectShape::Indexed = shape {
        server
            .request::<ContextMapRefreshResponse>(|request_id| ClientRequest::ContextMapRefresh {
                request_id,
                params: ContextMapRefreshParams {
                    project_id: project.project.id.clone(),
                },
            })
            .await?;
    }
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
                goal: "Answer a lookup.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "proportional-guard-run".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record-gate",
                    "blackboard_record_batch",
                    &json!({"records": [{
                        "idempotencyKey": "release-gate-note",
                        "kind": "fact",
                        "content": "The release gate is build C7-42.",
                        "importance": "high",
                        "confidenceBasisPoints": 9000,
                        "verification": "unverified",
                        "rootPromotion": "candidate"
                    }]})
                    .to_string(),
                ),
                responses::ev_completed("record-gate-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "lookup-complete",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "completionDisposition": "noReusableLearning",
                        "result": "The release gate is build C7-42."
                    })
                    .to_string(),
                ),
                responses::ev_completed("lookup-complete-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "The release gate is build C7-42."),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "What is the release gate?".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id),
                thread_id: None,
            },
        })
        .await?;
    Ok((
        requests[2]
            .function_call_output("lookup-complete")
            .to_string(),
        read.run.expect("run remains readable").status,
    ))
}

#[tokio::test]
async fn user_confirmation_does_not_hide_the_runs_own_write() -> Result<()> {
    let (responses_server, _codex_home, _project_root, mut server, project_id) =
        indexed_project("confirmed-write").await?;
    let (thread_id, run_id, run_revision) =
        start_lookup_run(&mut server, &project_id, "confirmed-write-run").await?;
    let record_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record",
                    "blackboard_record_batch",
                    &release_gate_batch(),
                ),
                responses::ev_completed("record-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("recorded", "Recorded the gate."),
                responses::ev_completed("recorded-response"),
            ]),
        ],
    )
    .await;
    run_lookup_turn(&mut server, &thread_id).await?;
    let recorded: Value = serde_json::from_str(
        &record_log
            .function_call_output_text("record")
            .expect("record output should be text"),
    )?;
    let entry_id = recorded["results"][0]["entryId"]
        .as_str()
        .expect("entry recorded")
        .to_string();
    let _: BlackboardConfirmResponse = server
        .request(|request_id| ClientRequest::BlackboardConfirm {
            request_id,
            params: BlackboardConfirmParams {
                project_id: project_id.clone(),
                entry_id,
                expected_revision: 1,
            },
        })
        .await?;

    let output =
        complete_without_learning(&responses_server, &mut server, &thread_id, run_revision).await?;

    assert!(
        output.contains("agent-written project knowledge changed in this project during this run")
    );
    assert_eq!(
        run_status(&mut server, run_id).await?,
        StatefulRunStatus::Running
    );
    Ok(())
}

#[tokio::test]
async fn another_runs_agent_write_conservatively_requires_durable_learning() -> Result<()> {
    let (responses_server, _codex_home, _project_root, mut server, project_id) =
        indexed_project("other-run-write").await?;
    let (writer_thread, _, _) =
        start_lookup_run(&mut server, &project_id, "other-run-writer").await?;
    let (lookup_thread, lookup_run, lookup_revision) =
        start_lookup_run(&mut server, &project_id, "other-run-lookup").await?;
    responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record",
                    "blackboard_record_batch",
                    &release_gate_batch(),
                ),
                responses::ev_completed("record-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("recorded", "Recorded the gate."),
                responses::ev_completed("recorded-response"),
            ]),
        ],
    )
    .await;
    run_lookup_turn(&mut server, &writer_thread).await?;

    let output = complete_without_learning(
        &responses_server,
        &mut server,
        &lookup_thread,
        lookup_revision,
    )
    .await?;

    assert!(
        output.contains("agent-written project knowledge changed in this project during this run")
    );
    assert_eq!(
        run_status(&mut server, lookup_run).await?,
        StatefulRunStatus::Running
    );
    Ok(())
}

fn release_gate_batch() -> String {
    json!({"records": [{
        "idempotencyKey": "release-gate-note",
        "kind": "fact",
        "content": "The release gate is build C7-42.",
        "importance": "high",
        "confidenceBasisPoints": 9000,
        "verification": "unverified",
        "rootPromotion": "candidate"
    }]})
    .to_string()
}

/// A project with an indexed (empty) root, so blackboard records succeed.
async fn indexed_project(
    name: &str,
) -> Result<(
    wiremock::MockServer,
    TempDir,
    TempDir,
    TestAppServer,
    String,
)> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
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
                name: name.to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: format!("{name}-project"),
            },
        })
        .await?;
    server
        .request::<ContextMapRefreshResponse>(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: project.project.id.clone(),
            },
        })
        .await?;
    Ok((
        responses_server,
        codex_home,
        project_root,
        server,
        project.project.id,
    ))
}

async fn start_lookup_run(
    server: &mut TestAppServer,
    project_id: &str,
    key: &str,
) -> Result<(String, String, u64)> {
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.to_string()),
            ..Default::default()
        })
        .await?;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project_id.to_string(),
                thread_id: thread.thread.id.clone(),
                goal: "Answer a lookup.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: key.to_string(),
            },
        })
        .await?;
    Ok((thread.thread.id, started.run.id, started.run.revision))
}

async fn run_lookup_turn(server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: "What is the release gate?".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

/// Runs one turn in which the model completes with `noReusableLearning`, returning the
/// completion tool output.
async fn complete_without_learning(
    responses_server: &wiremock::MockServer,
    server: &mut TestAppServer,
    thread_id: &str,
    expected_revision: u64,
) -> Result<String> {
    let log = responses::mount_sse_sequence(
        responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "lookup-complete",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": expected_revision,
                        "status": "completed",
                        "completionDisposition": "noReusableLearning",
                        "result": "The release gate is build C7-42."
                    })
                    .to_string(),
                ),
                responses::ev_completed("lookup-complete-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "The release gate is build C7-42."),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;
    run_lookup_turn(server, thread_id).await?;
    Ok(log
        .function_call_output_text("lookup-complete")
        .expect("completion output should be text"))
}

async fn run_status(server: &mut TestAppServer, run_id: String) -> Result<StatefulRunStatus> {
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id),
                thread_id: None,
            },
        })
        .await?;
    Ok(read.run.expect("run remains readable").status)
}
