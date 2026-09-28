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
use codex_app_server_protocol::StatefulAttributionCompletedNotification;
use codex_app_server_protocol::StatefulAttributionCounters;
use codex_app_server_protocol::StatefulAttributionStatus;
use codex_app_server_protocol::StatefulMeasurementListParams;
use codex_app_server_protocol::StatefulMeasurementListResponse;
use codex_app_server_protocol::StatefulMeasurementSummary;
use codex_app_server_protocol::StatefulMeasurementSummaryParams;
use codex_app_server_protocol::StatefulMeasurementSummaryResponse;
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
use codex_app_server_protocol::StatefulTurnStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::SteeringListParams;
use codex_app_server_protocol::SteeringListResponse;
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::SteeringUpdatedNotification;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn active_run_persists_terminal_attribution_and_trajectory() -> Result<()> {
    let responses = responses::start_mock_server().await;
    let completed = json!({
        "type": "response.completed",
        "response": {
            "id": "resp-1",
            "usage": {
                "input_tokens": 30,
                "input_tokens_details": {
                    "cached_tokens": 11,
                    "cache_write_tokens": 2
                },
                "output_tokens": 7,
                "output_tokens_details": { "reasoning_tokens": 3 },
                "total_tokens": 37
            }
        }
    });
    let body = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_assistant_message("msg-1", "Done"),
        completed,
    ]);
    let _response_mock = responses::mount_sse_once(&responses, body).await;
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
                name: "Measured run".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "measured-run-project".to_string(),
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
                goal: "Measure one turn.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 600,
                },
                idempotency_key: "measured-run".to_string(),
            },
        })
        .await?;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Complete one measured turn.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

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
    assert_eq!(measurements.next_cursor, None);
    let [measurement] = measurements.data.as_slice() else {
        panic!("expected one persisted turn measurement");
    };
    assert_eq!(measurement.run_id, started.run.id);
    assert_eq!(measurement.thread_id, thread.thread.id);
    assert_eq!(measurement.status, StatefulTurnStatus::Completed);
    assert_eq!(measurement.counters.world_state_samples, 2);
    assert_eq!(
        measurement.token_usage,
        Some(TokenUsageBreakdown {
            total_tokens: 37,
            input_tokens: 30,
            cached_input_tokens: 11,
            cache_write_input_tokens: 2,
            output_tokens: 7,
            reasoning_output_tokens: 3,
        })
    );
    assert_eq!(
        measurement
            .trajectory
            .as_ref()
            .expect("terminal trajectory persisted")
            .completed_model_responses,
        1
    );
    let summary: StatefulMeasurementSummaryResponse = server
        .request(|request_id| ClientRequest::StatefulMeasurementSummary {
            request_id,
            params: StatefulMeasurementSummaryParams {
                project_id: project.project.id.clone(),
                limit: Some(5),
            },
        })
        .await?;
    assert_eq!(
        summary.summary,
        StatefulMeasurementSummary {
            project_id: project.project.id,
            measurement_count: 1,
            run_count: 1,
            terminal_measurement_count: 1,
            completed_turns: 1,
            failed_turns: 0,
            aborted_turns: 0,
            turns_with_token_usage: 1,
            duration_ms: measurement.duration_ms,
            counters: measurement.counters.clone(),
            trajectory: measurement.trajectory.clone(),
            token_usage: measurement.token_usage.clone(),
            oldest_created_at: Some(measurement.created_at),
            newest_created_at: Some(measurement.created_at),
            has_more: false,
        }
    );
    Ok(())
}

#[tokio::test]
async fn selected_project_provides_shared_prompt_cache_affinity() -> Result<()> {
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
                name: "Shared project cache".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "shared-project-cache".to_string(),
            },
        })
        .await?;
    let expected_cache_key = format!("stateful-project:{}", project.project.id);

    for (response_id, message_id) in [
        ("first-response", "first-message"),
        ("second-response", "second-message"),
    ] {
        let request = responses::mount_sse_once(
            &responses_server,
            responses::sse(vec![
                responses::ev_response_created(response_id),
                responses::ev_assistant_message(message_id, "Done"),
                responses::ev_completed(response_id),
            ]),
        )
        .await;
        let thread = server
            .start_thread(ThreadStartParams {
                project_id: Some(project.project.id.clone()),
                ..Default::default()
            })
            .await?;
        server
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: thread.thread.id,
                input: vec![UserInput::Text {
                    text: "Use the selected project.".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;

        let request = request.single_request();
        assert_eq!(
            request.body_json()["prompt_cache_key"].as_str(),
            Some(expected_cache_key.as_str())
        );
        assert_eq!(
            request.header("session-id"),
            Some(expected_cache_key.clone())
        );
    }
    Ok(())
}

#[tokio::test]
async fn completion_rejects_an_unselected_source_fingerprint_without_mutating_the_run() -> Result<()>
{
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
                name: "Completion provenance guard".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "completion-provenance-project".to_string(),
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
                goal: "Complete with exact provenance.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "completion-provenance-run".to_string(),
            },
        })
        .await?;
    let invented = format!("sha256:{}", "a".repeat(64));
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "invalid-completion",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": 1,
                        "status": "completed",
                        "result": format!("The historical source was {invented}."),
                        "rootRevision": 0,
                        "materialRootFindings": [],
                        "completionIdempotencyKey": "invented-provenance",
                        "finalObligation": {
                            "implication": ["The result would otherwise be complete."],
                            "uncertainty": [format!("Historical evidence used {invented} without a selected provenance record.")]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("invalid-completion-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "provenance-rejected",
                    "The completion was rejected because its provenance was not selected.",
                ),
                responses::ev_completed("provenance-rejected-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Finish with the historical source fingerprint.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    let rejection = requests[1].function_call_output("invalid-completion");
    assert!(rejection.to_string().contains("unknown source fingerprint"));
    assert!(rejection.to_string().contains("materialHistoricalFindings"));
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id),
                thread_id: None,
            },
        })
        .await?;
    let run = read.run.expect("run remains readable after rejection");
    assert_eq!(run.status, StatefulRunStatus::Running);
    assert_eq!(run.result, None);
    Ok(())
}

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
    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let project_node_id = HierarchyNodeId::parse(format!("project-node-{}", project.project.id))?;
    HierarchyStore::open(&sqlite)
        .await?
        .create_node(
            project_node_id.clone(),
            NewHierarchyNode {
                project_id: project.project.id.clone(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    let material_finding_id =
        BlackboardEntryId::parse(format!("material-finding-{}", project.project.id))?;
    let blackboard = BlackboardStore::open(&sqlite).await?;
    blackboard
        .create_entry(
            material_finding_id.clone(),
            NewBlackboardEntry {
                project_id: project.project.id.clone(),
                node_id: project_node_id,
                kind: BlackboardKind::Fact,
                content: "The decisive project constraint must remain in the durable result."
                    .to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000)?,
                verification: BlackboardVerification::UserConfirmed,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "completion-integration-fixture".to_string(),
                },
            },
        )
        .await?;
    let root_revision = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: project.project.id.clone(),
            max_entries: 256,
        })
        .await?
        .revision;
    let material_finding_reference = "E1";
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
                        "result": "Verified the decisive connection and incorporated the user's direction.",
                        "rootRevision": root_revision,
                        "materialRootFindings": [material_finding_reference],
                        "completionIdempotencyKey": "final-decisive-connection",
                        "finalObligation": {
                            "examined": ["The verified source constraint and deployment finding."],
                            "learning": ["The deployment risk is triggered by the source constraint."],
                            "implication": ["The decisive project constraint must remain in the durable result."],
                            "uncertainty": ["No material uncertainty remains for this connection."],
                            "blockers": ["No blocker remains."]
                        }
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
            thread_id: thread.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Continue and incorporate my steering.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let first_obligation_event: ObligationUpdatedNotification =
        server.read_notification("obligation/updated").await?;
    let final_obligation_event: ObligationUpdatedNotification =
        server.read_notification("obligation/updated").await?;
    assert_eq!(first_obligation_event.run_id, started.run.id);
    assert_eq!(first_obligation_event.revision, 1);
    assert_eq!(final_obligation_event.run_id, started.run.id);
    assert_eq!(final_obligation_event.revision, 1);
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
    let attribution: StatefulAttributionCompletedNotification = server
        .read_notification("statefulAttribution/completed")
        .await?;
    assert_eq!(attribution.project_id, started.run.project_id);
    assert_eq!(attribution.thread_id, thread.thread.id);
    assert_eq!(attribution.status, StatefulAttributionStatus::Completed);
    assert!(attribution.duration_ms > 0);
    assert!(attribution.counters.world_state_samples > 0);
    assert!(attribution.counters.root_entries_loaded > 0);
    assert_eq!(
        attribution.counters,
        StatefulAttributionCounters {
            world_state_samples: attribution.counters.world_state_samples,
            root_entries_loaded: attribution.counters.root_entries_loaded,
            stateful_tool_calls: 4,
            obligation_write_calls: 1,
            run_update_calls: 1,
            steering_write_calls: 2,
            material_findings_reused: 1,
            ..Default::default()
        }
    );

    let requests = response_log.requests();
    assert_eq!(requests.len(), 5);
    assert!(requests[0].body_contains_text("<stateful_run>"));
    assert!(requests[0].body_contains_text(
        "do not turn one criterion or decision dimension into an overall project determination"
    ));
    assert!(requests[0].body_contains_text("removes the active-run binding"));
    assert!(requests[0].body_contains_text("final Stateful mutation"));
    assert!(requests[0].body_contains_text("rootRevision"));
    assert!(
        requests[0].body_contains_text(&format!("Project intelligence revision: {root_revision}"))
    );
    assert!(requests[0].body_contains_text(material_finding_reference));
    assert!(requests[0].body_contains_text("Connect the source constraint to deployment risk."));
    assert!(requests[0].body_contains_text(&submitted.steering.id));
    assert!(requests[4].body_contains_text("finalAnswerChecklist"));
    assert!(requests[4].body_contains_text("submittedResult"));
    assert!(requests[4].body_contains_text(
        "Verified the decisive connection and incorporated the user's direction."
    ));
    assert!(requests[4].body_contains_text("rootFinding"));
    assert!(requests[4].body_contains_text(material_finding_reference));
    assert!(
        requests[4].body_contains_text(
            "The decisive project constraint must remain in the durable result."
        )
    );
    assert!(
        requests[4]
            .body_contains_text("The deployment risk is triggered by the source constraint.")
    );
    assert!(requests[4].body_contains_text("Return submittedResult as the final answer"));
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
    assert_eq!(obligations.data.len(), 2);
    assert!(obligations.data.iter().any(|obligation| {
        obligation.packet.learning
            == vec!["The deployment risk is triggered by the source constraint."]
            && obligation.packet.blockers == vec!["No blocker remains."]
    }));
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
    let run = read.run.expect("completed run remains readable");
    assert_eq!(run.status, StatefulRunStatus::Completed);
    let result = run.result.expect("completed run preserves its result");
    assert!(result.contains("Verified the decisive connection"));
    assert!(result.contains("Durable completion basis:"));
    assert!(result.contains(material_finding_reference));
    assert!(result.contains("The decisive project constraint must remain in the durable result."));
    assert!(result.contains("The deployment risk is triggered by the source constraint."));
    Ok(())
}

#[tokio::test]
async fn model_cannot_persist_final_packet_as_intermediate_obligation() -> Result<()> {
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
                name: "Intermediate obligation gate".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "intermediate-obligation-project".to_string(),
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
                goal: "Return the completed finding without a redundant update.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "intermediate-obligation-run".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "empty-future-obligation",
                    "obligation_update",
                    &json!({
                        "idempotencyKey": "redundant-final-packet",
                        "packet": {
                            "next": ["Synthesize the already-reviewed evidence into the final answer."]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("empty-future-obligation-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(
                    "gate-observed-message",
                    "The final packet must be submitted with completion.",
                ),
                responses::ev_completed("gate-observed-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Finish the ready result.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body_contains_text("meaningful semantic content"));
    assert!(requests[0].body_contains_text("requestedJudgment"));
    assert!(
        requests[1]
            .body_contains_text("intermediate obligation_update requires meaningful learning")
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
async fn model_reads_a_long_goal_exactly_and_rejects_a_foreign_cursor() -> Result<()> {
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
                name: "Long goal".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "long-goal-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let goal = format!(
        "{} Binding constraint: never cite the \"draft\" schedule.",
        "Review every clause of the purchase agreement. ".repeat(300)
    );
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: goal.clone(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "long-goal-run".to_string(),
            },
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "read-goal",
                    "stateful_run_read",
                    &json!({"section": "goal"}).to_string(),
                ),
                responses::ev_completed("read-goal-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "foreign-cursor",
                    "stateful_run_read",
                    &json!({"section": "goal", "cursor": "goal.run-elsewhere.00ff.10"}).to_string(),
                ),
                responses::ev_completed("foreign-cursor-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "Read the goal."),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Start the review.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    let first_page: Value = serde_json::from_str(
        requests[1].function_call_output("read-goal")["output"]
            .as_str()
            .expect("tool output text"),
    )?;
    let content = first_page["content"].as_str().expect("page content");
    assert!(first_page.to_string().len() <= 9_000);
    assert!(goal.starts_with(content) && content.len() < goal.len());
    assert_eq!(first_page["totalBytes"], json!(goal.len()));
    assert_eq!(first_page["runId"], json!(started.run.id));
    assert!(
        first_page["nextCursor"]
            .as_str()
            .is_some_and(|cursor| cursor.ends_with(&format!(".{}", content.len())))
    );
    assert!(
        requests[2]
            .function_call_output("foreign-cursor")
            .to_string()
            .contains("cursor does not belong to a run of the selected thread")
    );
    Ok(())
}

#[tokio::test]
async fn oversized_completion_commits_once_and_pages_the_result_exactly() -> Result<()> {
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
                name: "Oversized result".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "oversized-result-project".to_string(),
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
                goal: "Summarize the data room.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "oversized-result-run".to_string(),
            },
        })
        .await?;
    let result = "The \"indemnity\" cap is 15%; the DOE renewal is unresolved.\n"
        .repeat(300)
        .trim_end()
        .to_string();
    let complete_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "complete",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "completionDisposition": "noReusableLearning",
                        "result": result,
                    })
                    .to_string(),
                ),
                responses::ev_completed("complete-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("completed", "Completed."),
                responses::ev_completed("completed-response"),
            ]),
        ],
    )
    .await;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Finish.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let completion_text = complete_log
        .function_call_output_text("complete")
        .expect("completion output should be text");
    let completion: Value = serde_json::from_str(&completion_text)?;
    assert!(completion_text.len() <= 9_000);
    assert_eq!(
        (&completion["status"], &completion["submittedResult"]),
        (&json!("completed"), &Value::Null)
    );
    let cursor = completion["submittedResultCursor"]
        .as_str()
        .expect("oversized result is paged")
        .to_string();
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id),
                thread_id: None,
            },
        })
        .await?;
    let run = read.run.expect("run remains readable");
    assert_eq!(
        (run.status, run.result.as_deref()),
        (StatefulRunStatus::Completed, Some(result.as_str()))
    );

    let read_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "read-result",
                    "stateful_run_read",
                    &json!({"section": "submittedResult", "cursor": cursor}).to_string(),
                ),
                responses::ev_completed("read-result-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("read-done", "Read the result."),
                responses::ev_completed("read-done-response"),
            ]),
        ],
    )
    .await;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Show the result.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let page: Value = serde_json::from_str(
        &read_log
            .function_call_output_text("read-result")
            .expect("read output should be text"),
    )?;
    let content = page["content"].as_str().expect("page content");
    assert!(page.to_string().len() <= 9_000);
    assert!(result.starts_with(content) && !content.is_empty());
    assert!(page["nextCursor"].is_string());
    Ok(())
}
