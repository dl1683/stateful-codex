#[cfg(not(target_os = "windows"))]
use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::ObligationUpdatedNotification;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::ProjectRoot;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulAttributionCompletedNotification;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulAttributionCounters;
#[cfg(not(target_os = "windows"))]
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
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulSteeringStatus;
use codex_app_server_protocol::StatefulTurnStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::SteeringListParams;
use codex_app_server_protocol::SteeringListResponse;
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::SteeringUpdatedNotification;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardEntryId;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardImportance;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardKind;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardProvenance;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardProvenanceKind;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardStore;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardVerification;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::ConfidenceScore;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::HierarchyNodeId;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::HierarchyStore;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::NewBlackboardEntry;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::NewHierarchyNode;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::NodeKind;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::ProjectRelativePath;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::RootBlackboardQuery;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::RootPromotion;
#[cfg(not(target_os = "windows"))]
use codex_state::SqliteConfig;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::ObligationPacket;
#[cfg(not(target_os = "windows"))]
use codex_utils_absolute_path::AbsolutePathBuf;
#[cfg(not(target_os = "windows"))]
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
#[cfg(not(target_os = "windows"))]
use sqlx::SqlitePool;
#[cfg(not(target_os = "windows"))]
use sqlx::sqlite::SqliteConnectOptions;
#[cfg(not(target_os = "windows"))]
use sqlx::sqlite::SqlitePoolOptions;
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
                        "openIssues": [],
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
    assert!(rejection.to_string().contains("materialRootFindings"));
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

/// The applied steering joins the acceptance request, so the run completes only after a
/// criterion quoting it is checked (`sh` runs the checker, so not on Windows).
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn model_updates_semantic_progress_and_applies_user_steering() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("report.md"),
        "The deployment risk is triggered by the source constraint.\n",
    )?;
    std::fs::write(project_root.path().join("verify.sh"), "test -s report.md\n")?;
    super::stateful_acceptance_support::init_repository(project_root.path())?;
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
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
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
                    "declare-acceptance",
                    "stateful_acceptance_update",
                    &json!({
                        "expectedLedgerRevision": 0,
                        "changes": [
                            {"action": "add", "origin": "user", "kind": "deliverable", "statement": "The decisive source connection is found and verified.", "requestQuote": "Find and verify the decisive source connection.", "checkCommand": "sh verify.sh", "expectedObservation": "the report exists", "artifacts": ["report.md"], "checker": ["verify.sh"]},
                            {"action": "add", "origin": "user", "kind": "deliverable", "statement": "The report connects the constraint to deployment risk.", "requestQuote": "Connect the source constraint to deployment risk.", "checkCommand": "sh verify.sh", "expectedObservation": "the report exists", "artifacts": ["report.md"], "checker": ["verify.sh"]}
                        ]
                    })
                    .to_string(),
                ),
                responses::ev_completed("declare-acceptance-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "reconcile-acceptance",
                    "stateful_acceptance_update",
                    &json!({
                        "expectedLedgerRevision": 1,
                        "changes": [
                            {"action": "reconcileSteering", "steeringId": submitted.steering.id.clone(), "text": "The direction adds C2."},
                            {"action": "admit", "criterion": "C1"},
                            {"action": "admit", "criterion": "C2"}
                        ]
                    })
                    .to_string(),
                ),
                responses::ev_completed("reconcile-acceptance-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "run-check",
                    "exec_command",
                    &json!({
                        "cmd": "sh verify.sh",
                        "workdir": project_root.path().to_string_lossy(),
                        "yield_time_ms": 10_000
                    })
                    .to_string(),
                ),
                responses::ev_completed("run-check-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "complete-run",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": 2,
                        "status": "completed",
                        "openIssues": [],
                        "result": "Verified the decisive connection and incorporated the user's direction.",
                        "rootRevision": root_revision,
                        "materialRootFindings": [material_finding_reference],
                        "completionIdempotencyKey": "final-decisive-connection",
                        "finalObligation": {
                            "examined": ["The verified source constraint and deployment finding."],
                            "learning": ["The deployment risk is triggered by the source constraint."],
                            "implication": ["The decisive project constraint must remain in the durable result."],
                            "uncertainty": ["No material uncertainty remains for this connection."]
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
    assert_eq!(requests.len(), 8);
    assert!(requests[0].body_contains_text("<stateful_run>"));
    assert!(requests[0].body_contains_text("check only what the task depends on"));
    assert!(requests[0].body_contains_text("completed only after all other durable writes"));
    assert!(requests[0].body_contains_text("final Stateful mutation"));
    assert!(requests[0].body_contains_text("rootRevision"));
    assert!(
        requests[0].body_contains_text(&format!("Project intelligence revision: {root_revision}"))
    );
    assert!(requests[0].body_contains_text(material_finding_reference));
    assert!(requests[0].body_contains_text("Connect the source constraint to deployment risk."));
    assert!(requests[0].body_contains_text(&submitted.steering.id));
    assert!(requests[7].body_contains_text("finalAnswerChecklist"));
    assert!(requests[7].body_contains_text("submittedResult"));
    assert!(requests[7].body_contains_text(
        "Verified the decisive connection and incorporated the user's direction."
    ));
    assert!(requests[7].body_contains_text("rootFinding"));
    assert!(requests[7].body_contains_text(material_finding_reference));
    assert!(
        requests[7].body_contains_text(
            "The decisive project constraint must remain in the durable result."
        )
    );
    assert!(
        requests[7]
            .body_contains_text("The deployment risk is triggered by the source constraint.")
    );
    assert!(requests[7].body_contains_text("Return submittedResult as the final answer"));
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
            && obligation.packet.blockers.is_empty()
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
    assert!(result.contains("reconciled by the agent (agent-written, not host-verified)"));
    assert!(result.contains("ran its host-admitted plan"));
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
                    &json!({"section": "goal", "cursor": "v1.goal.run-elsewhere.0123456789abcdef0123456789abcdef.10.0"}).to_string(),
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
async fn run_read_rejects_stale_and_tampered_cursors_at_the_handler() -> Result<()> {
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
                name: "Exact cursor validation".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "exact-cursor-project".to_string(),
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
                goal: format!("🙂{}", "goal detail ".repeat(1_000))
                    .trim_end()
                    .to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "exact-cursor-run".to_string(),
            },
        })
        .await?;

    let strategy = format!("strategy detail {}", "x ".repeat(5_000))
        .trim_end()
        .to_string();
    let strategy_update: Value = serde_json::from_str(
        &run_model_tool(
            &responses_server,
            &mut server,
            &thread.thread.id,
            "set-strategy-for-cursor",
            "stateful_run_update",
            &json!({
                "expectedRevision": started.run.revision,
                "status": "running",
                "strategy": strategy,
            }),
        )
        .await?,
    )?;
    let strategy_revision = strategy_update["revision"]
        .as_u64()
        .expect("strategy update returns a run revision");

    let goal_page: Value = serde_json::from_str(
        &run_model_tool(
            &responses_server,
            &mut server,
            &thread.thread.id,
            "read-goal-for-cursor",
            "stateful_run_read",
            &json!({"section": "goal"}),
        )
        .await?,
    )?;
    let goal_cursor = goal_page["nextCursor"]
        .as_str()
        .expect("long goal returns a cursor")
        .to_string();
    let strategy_page: Value = serde_json::from_str(
        &run_model_tool(
            &responses_server,
            &mut server,
            &thread.thread.id,
            "read-strategy-for-cursor",
            "stateful_run_read",
            &json!({"section": "strategy"}),
        )
        .await?,
    )?;
    let strategy_cursor = strategy_page["nextCursor"]
        .as_str()
        .expect("long strategy returns a cursor")
        .to_string();
    let packet_items = (0..12)
        .map(|index| {
            format!("learning item {index}: {}", "detail ".repeat(110))
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    run_model_tool(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "write-obligation-for-cursor",
        "obligation_update",
        &json!({
            "idempotencyKey": "cursor-obligation-1",
            "packet": {
                "learning": packet_items,
                "next": ["Read every cursor page before continuing."],
            },
        }),
    )
    .await?;
    let obligation_page: Value = serde_json::from_str(
        &run_model_tool(
            &responses_server,
            &mut server,
            &thread.thread.id,
            "read-obligation-for-cursor",
            "stateful_run_read",
            &json!({"section": "obligation"}),
        )
        .await?,
    )?;
    let obligation_cursor = obligation_page["nextCursor"]
        .as_str()
        .expect("long obligation returns a cursor")
        .to_string();

    run_model_tool(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "stale-strategy-update",
        "stateful_run_update",
        &json!({
            "expectedRevision": strategy_revision,
            "status": "running",
            "strategy": "new strategy after cursor issuance",
        }),
    )
    .await?;
    run_model_tool(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "stale-obligation-update",
        "obligation_update",
        &json!({
            "idempotencyKey": "cursor-obligation-2",
            "packet": {
                "learning": ["new obligation after cursor issuance"],
                "next": ["Use only the current obligation after the cursor test."],
            },
        }),
    )
    .await?;

    let goal_parts = goal_cursor.split('.').collect::<Vec<_>>();
    assert_eq!(goal_parts.len(), 6);
    let mut wrong_digest = goal_parts.clone();
    wrong_digest[3] = "00000000000000000000000000000000";
    let mut wrong_length = goal_parts.clone();
    wrong_length[4] = "1";
    let mut past_end = goal_parts
        .iter()
        .map(|part| (*part).to_string())
        .collect::<Vec<_>>();
    past_end[5] = goal_parts[4]
        .parse::<usize>()?
        .saturating_add(1)
        .to_string();
    let cases = [
        (
            "stale strategy",
            "strategy",
            strategy_cursor,
            "changed after this cursor was issued",
        ),
        (
            "stale obligation",
            "obligation",
            obligation_cursor,
            "changed after this cursor was issued",
        ),
        (
            "wrong digest",
            "goal",
            wrong_digest.join("."),
            "changed after this cursor was issued",
        ),
        (
            "wrong length",
            "goal",
            wrong_length.join("."),
            "changed after this cursor was issued",
        ),
        (
            "section mismatch",
            "strategy",
            goal_cursor.clone(),
            "not a strategy cursor",
        ),
        (
            "offset past end",
            "goal",
            past_end.join("."),
            "not a valid position",
        ),
        (
            "offset inside multibyte scalar",
            "goal",
            [goal_parts[..5].join("."), "1".to_string()].join("."),
            "not a valid position",
        ),
    ];
    for (label, section, cursor, expected) in cases {
        let output = run_model_tool(
            &responses_server,
            &mut server,
            &thread.thread.id,
            &format!("tampered-{label}"),
            "stateful_run_read",
            &json!({"section": section, "cursor": cursor}),
        )
        .await?;
        assert!(output.contains(expected), "{label}: {output}");
    }

    let foreign_thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id),
            ..Default::default()
        })
        .await?;
    let output = run_model_tool(
        &responses_server,
        &mut server,
        &foreign_thread.thread.id,
        "other-thread-cursor",
        "stateful_run_read",
        &json!({"section": "goal", "cursor": goal_cursor}),
    )
    .await?;
    assert!(output.contains("cursor does not belong to a run of the selected thread"));
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn oversized_completion_commits_once_and_pages_the_result_exactly() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::write_acceptance_files(project_root.path())?;
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
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
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
    super::stateful_acceptance_support::seed_admitted_plan(codex_home.path(), &started.run.id, &[])
        .await?;
    let result = "The \"indemnity\" cap is 15%; the DOE renewal is unresolved.\n"
        .repeat(300)
        .trim_end()
        .to_string();
    let complete_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            super::stateful_acceptance_support::run_check("acceptance-check", project_root.path()),
            responses::sse(vec![
                responses::ev_function_call(
                    "complete",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "openIssues": [],
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
    let stored = run.result.unwrap_or_default();
    assert_eq!(run.status, StatefulRunStatus::Completed);
    // The durable result is the submitted result followed by its acceptance basis.
    assert!(stored.starts_with(&format!(
        "{result}

Acceptance basis:"
    )));

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

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn oversized_durable_completion_pages_result_and_final_obligation_exactly() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::write_acceptance_files(project_root.path())?;
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
                name: "Oversized durable completion".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "oversized-durable-project".to_string(),
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
    let blackboard = BlackboardStore::open(&sqlite).await?;
    blackboard
        .create_entry(
            BlackboardEntryId::parse(format!("material-finding-{}", project.project.id))?,
            NewBlackboardEntry {
                project_id: project.project.id.clone(),
                node_id: project_node_id,
                kind: BlackboardKind::Fact,
                content: "The indemnity cap is 15% of the purchase price.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000)?,
                verification: BlackboardVerification::UserConfirmed,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "oversized-durable-fixture".to_string(),
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
                goal: "Write the red-flag memo.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "oversized-durable-run".to_string(),
            },
        })
        .await?;
    super::stateful_acceptance_support::seed_admitted_plan(codex_home.path(), &started.run.id, &[])
        .await?;
    let result =
        "The \"indemnity\" cap is 15%; the DOE renewal remains unresolved. “Priority 1.”\n"
            .repeat(150)
            .trim_end()
            .to_string();
    let learning = (0..15)
        .map(|index| {
            format!(
                "Learning {index}: {}",
                "The cap binds every seller claim. ".repeat(22).trim_end()
            )
        })
        .collect::<Vec<_>>();
    let complete_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            super::stateful_acceptance_support::run_check("acceptance-check", project_root.path()),
            responses::sse(vec![
                responses::ev_function_call(
                    "complete",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "completed",
                        "openIssues": [],
                        "result": result,
                        "rootRevision": root_revision,
                        "materialRootFindings": ["E1"],
                        "completionIdempotencyKey": "oversized-durable-completion",
                        "finalObligation": {"learning": learning}
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
    run_simple_turn(&mut server, &thread.thread.id).await?;
    let completion_text = complete_log
        .function_call_output_text("complete")
        .expect("completion output should be text");
    let completion: Value = serde_json::from_str(&completion_text)?;
    assert!(completion_text.len() <= 9_000, "{}", completion_text.len());
    assert_eq!(
        (
            &completion["status"],
            &completion["submittedResult"],
            completion["omittedChecklistItems"]
                .as_u64()
                .is_some_and(|omitted| omitted > 0),
        ),
        (&json!("completed"), &Value::Null, true)
    );

    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id),
                thread_id: None,
            },
        })
        .await?;
    let stored = read
        .run
        .expect("run remains readable")
        .result
        .expect("stored result");
    let bounded_learning = learning
        .iter()
        .map(|item| format!("\n- Learning: {}…", &item[..637]))
        .collect::<String>();
    // The finding is the user's own memory: the basis names it without quoting its words.
    let durable_suffix = format!(
        "\n\nDurable completion basis:\n- Root finding: E1 [critical; verification=userConfirmed; evidence=notApplicable; premises=notApplicable] (user-stated; content as shown in the root packet){bounded_learning}"
    );
    assert!(stored.starts_with(&format!("{result}{durable_suffix}")));
    assert!(stored.contains("Acceptance basis:"));

    let paged_result = read_every_page(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "submittedResult",
        Some(
            completion["submittedResultCursor"]
                .as_str()
                .expect("result cursor"),
        ),
    )
    .await?;
    assert_eq!(paged_result, result);
    let paged_obligation = read_every_page(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "obligation",
        Some(
            completion["finalObligationCursor"]
                .as_str()
                .expect("obligation cursor"),
        ),
    )
    .await?;
    let expected_packet = ObligationPacket {
        learning,
        ..Default::default()
    };
    assert_eq!(
        serde_json::from_str::<ObligationPacket>(&paged_obligation)?,
        expected_packet
    );
    let omitted = completion["omittedChecklistItems"]
        .as_u64()
        .expect("omitted checklist count");
    let returned_checklist = completion["finalAnswerChecklist"]
        .as_array()
        .expect("final checklist")
        .len() as u64;
    assert!(omitted > 0);
    assert_eq!(omitted, 16 - returned_checklist);
    assert!(completion["finalObligationCursor"].is_string());
    Ok(())
}

#[cfg(not(target_os = "windows"))]
async fn completion_fence_fixture(
    responses_server: &wiremock::MockServer,
) -> Result<(
    TempDir,
    TestAppServer,
    String,
    String,
    String,
    u64,
    u64,
    HierarchyNodeId,
)> {
    let codex_home = TempDir::new()?;
    // A project root holding only the files the completion's acceptance check pins.
    let project_root = codex_home.path().join("project");
    std::fs::create_dir(&project_root)?;
    super::stateful_acceptance_support::write_acceptance_files(&project_root)?;
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
                name: "Completion fence".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.clone())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "completion-fence-project".to_string(),
            },
        })
        .await?;
    let project_node_id = HierarchyNodeId::parse(format!("project-node-{}", project.project.id))?;
    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
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
    let root_revision = BlackboardStore::open(&sqlite)
        .await?
        .root_projection(RootBlackboardQuery {
            project_id: project.project.id.clone(),
            max_entries: 256,
        })
        .await?
        .revision;
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
                goal: "Prove completion ordering.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "completion-fence-run".to_string(),
            },
        })
        .await?;
    super::stateful_acceptance_support::seed_admitted_plan(codex_home.path(), &started.run.id, &[])
        .await?;
    responses::mount_sse_sequence(
        responses_server,
        vec![
            super::stateful_acceptance_support::run_check("acceptance-check", &project_root),
            responses::sse(vec![
                responses::ev_assistant_message("checked", "Checked."),
                responses::ev_completed("checked-response"),
            ]),
        ],
    )
    .await;
    run_simple_turn(&mut server, &thread.thread.id).await?;
    responses_server.reset().await;
    Ok((
        codex_home,
        server,
        project.project.id,
        thread.thread.id,
        started.run.id,
        started.run.revision,
        root_revision,
        project_node_id,
    ))
}

#[cfg(not(target_os = "windows"))]
fn agent_entry(
    project_id: &str,
    node_id: &HierarchyNodeId,
    id: &str,
) -> Result<NewBlackboardEntry> {
    Ok(NewBlackboardEntry {
        project_id: project_id.to_string(),
        node_id: node_id.clone(),
        kind: BlackboardKind::Fact,
        content: format!("Agent mutation {id}"),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(10_000)?,
        verification: BlackboardVerification::UserConfirmed,
        importance: BlackboardImportance::Normal,
        root_promotion: RootPromotion::NotPromoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::User,
            source_id: id.to_string(),
        },
    })
}

#[cfg(not(target_os = "windows"))]
async fn held_runtime_transaction(
    codex_home: &TempDir,
) -> Result<(SqlitePool, sqlx::Transaction<'static, sqlx::Sqlite>)> {
    let runtime_path = codex_home.path().join("stateful_runtime_1.sqlite");
    let runtime_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(runtime_path)
                .create_if_missing(false),
        )
        .await?;
    let transaction = runtime_pool.begin_with("BEGIN IMMEDIATE").await?;
    Ok((runtime_pool, transaction))
}

#[cfg(not(target_os = "windows"))]
async fn mount_delayed_sse_once(responses_server: &wiremock::MockServer, body: String) {
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path_regex(".*/responses$"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body)
                .set_delay(Duration::from_millis(200)),
        )
        .up_to_n_times(1)
        .mount(responses_server)
        .await;
}

#[cfg(not(target_os = "windows"))]
async fn wait_for_response_request(responses_server: &wiremock::MockServer) {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if !responses_server
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial model request arrives");
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn completion_holds_pi_fence_until_runtime_commit_before_agent_mutation() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (codex_home, mut server, project_id, thread_id, run_id, revision, root_revision, node_id) =
        completion_fence_fixture(&responses_server).await?;
    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let blackboard = BlackboardStore::open(&sqlite).await?;
    let completion_call = responses::sse(vec![
        responses::ev_function_call(
            "complete",
            "stateful_run_update",
            &json!({
                "expectedRevision": revision,
                "status": "completed",
                "openIssues": [],
                "result": "Completion committed.",
                "rootRevision": root_revision,
                "materialRootFindings": [],
                "completionIdempotencyKey": "completion-fence-success",
                "finalObligation": {"implication": ["Completion ordering is durable."]}
            })
            .to_string(),
        ),
        responses::ev_completed("complete-response"),
    ]);
    mount_delayed_sse_once(&responses_server, completion_call).await;
    let mut completion = Box::pin(server.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread_id.clone(),
        input: vec![UserInput::Text {
            text: "Complete the run.".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    }));
    tokio::select! {
        result = &mut completion => panic!("completion finished before the runtime lock: {result:?}"),
        () = wait_for_response_request(&responses_server) => {}
    }
    let (_runtime_pool, runtime_transaction) = held_runtime_transaction(&codex_home).await?;
    responses_server.reset().await;
    let completion_log = responses::mount_sse_sequence(
        &responses_server,
        vec![responses::sse(vec![
            responses::ev_assistant_message("completed", "Completed."),
            responses::ev_completed("completed-response"),
        ])],
    )
    .await;
    wait_until_completion_holds_fence(&blackboard).await?;
    let mutation = blackboard.create_entry(
        BlackboardEntryId::parse("completion-fence-agent-mutation")?,
        agent_entry(&project_id, &node_id, "completion-fence-agent-mutation")?,
    );
    tokio::pin!(mutation);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut mutation)
            .await
            .is_err()
    );
    runtime_transaction.commit().await?;
    completion.await?;
    let completion: Value = serde_json::from_str(
        &completion_log
            .function_call_output_text("complete")
            .expect("completion output"),
    )?;
    assert_eq!(completion["status"], json!("completed"));
    assert_eq!(mutation.await?.revision, 1);
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id),
                thread_id: None,
            },
        })
        .await?;
    assert_eq!(
        read.run.expect("completed run").status,
        StatefulRunStatus::Completed
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn completion_revision_conflict_releases_pi_fence_and_preserves_run() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (codex_home, mut server, project_id, thread_id, run_id, revision, root_revision, node_id) =
        completion_fence_fixture(&responses_server).await?;
    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let blackboard = BlackboardStore::open(&sqlite).await?;
    let completion_call = responses::sse(vec![
        responses::ev_function_call(
            "conflict",
            "stateful_run_update",
            &json!({
                "expectedRevision": revision,
                "status": "completed",
                "openIssues": [],
                "result": "Should not commit.",
                "rootRevision": root_revision,
                "materialRootFindings": [],
                "completionIdempotencyKey": "completion-fence-conflict",
                "finalObligation": {"implication": ["Conflict is expected."]}
            })
            .to_string(),
        ),
        responses::ev_completed("conflict-response"),
    ]);
    mount_delayed_sse_once(&responses_server, completion_call).await;
    let mut completion = Box::pin(server.start_turn_and_wait_for_completion(TurnStartParams {
        thread_id: thread_id.clone(),
        input: vec![UserInput::Text {
            text: "Complete the run.".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    }));
    tokio::select! {
        result = &mut completion => panic!("completion finished before the runtime lock: {result:?}"),
        () = wait_for_response_request(&responses_server) => {}
    }
    let (_runtime_pool, mut runtime_transaction) = held_runtime_transaction(&codex_home).await?;
    responses_server.reset().await;
    let completion_log = responses::mount_sse_sequence(
        &responses_server,
        vec![responses::sse(vec![
            responses::ev_assistant_message("conflict-done", "Conflict handled."),
            responses::ev_completed("conflict-done-response"),
        ])],
    )
    .await;
    wait_until_completion_holds_fence(&blackboard).await?;
    sqlx::query("UPDATE stateful_runs SET revision = revision + 1 WHERE id = ?")
        .bind(&run_id)
        .execute(&mut *runtime_transaction)
        .await?;
    runtime_transaction.commit().await?;
    completion.await?;
    assert!(
        completion_log
            .function_call_output_text("conflict")
            .expect("conflict output")
            .contains("revision conflict")
    );
    blackboard
        .acquire_completion_fence(Duration::from_millis(100))
        .await
        .expect("fence released")
        .release()
        .await
        .expect("fence releases");
    assert_eq!(
        blackboard
            .create_entry(
                BlackboardEntryId::parse("completion-fence-conflict-agent-mutation")?,
                agent_entry(
                    &project_id,
                    &node_id,
                    "completion-fence-conflict-agent-mutation"
                )?,
            )
            .await?
            .revision,
        1
    );
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id),
                thread_id: None,
            },
        })
        .await?;
    assert_eq!(
        read.run.expect("nonterminal run").status,
        StatefulRunStatus::Running
    );
    Ok(())
}

async fn run_simple_turn(server: &mut TestAppServer, thread_id: &str) -> Result<()> {
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

async fn run_model_tool(
    responses_server: &wiremock::MockServer,
    server: &mut TestAppServer,
    thread_id: &str,
    call_id: &str,
    tool_name: &str,
    arguments: &Value,
) -> Result<String> {
    let response_log = responses::mount_sse_sequence(
        responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(call_id, tool_name, &arguments.to_string()),
                responses::ev_completed(&format!("{call_id}-response")),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message(&format!("{call_id}-done"), "Done."),
                responses::ev_completed(&format!("{call_id}-done-response")),
            ]),
        ],
    )
    .await;
    run_simple_turn(server, thread_id).await?;
    Ok(response_log
        .function_call_output_text(call_id)
        .expect("tool output should be text"))
}

/// Follows a `stateful_run_read` cursor through every page, one model turn per page,
/// asserting each page fits the model item bound, and returns the concatenated content.
async fn read_every_page(
    responses_server: &wiremock::MockServer,
    server: &mut TestAppServer,
    thread_id: &str,
    section: &str,
    first_cursor: Option<&str>,
) -> Result<String> {
    let mut content = String::new();
    let mut cursor = first_cursor.map(str::to_string);
    let mut page_index = 0;
    loop {
        let call_id = format!("read-{section}-{page_index}");
        let log = responses::mount_sse_sequence(
            responses_server,
            vec![
                responses::sse(vec![
                    responses::ev_function_call(
                        &call_id,
                        "stateful_run_read",
                        &json!({"section": section, "cursor": cursor}).to_string(),
                    ),
                    responses::ev_completed(&format!("{call_id}-response")),
                ]),
                responses::sse(vec![
                    responses::ev_assistant_message(&format!("{call_id}-done"), "Read."),
                    responses::ev_completed(&format!("{call_id}-done-response")),
                ]),
            ],
        )
        .await;
        run_simple_turn(server, thread_id).await?;
        let text = log
            .function_call_output_text(&call_id)
            .expect("read output should be text");
        assert!(
            text.len() <= 9_000,
            "page {page_index} is {} bytes",
            text.len()
        );
        let page: Value = serde_json::from_str(&text)?;
        content.push_str(page["content"].as_str().expect("page content"));
        cursor = page["nextCursor"].as_str().map(str::to_string);
        page_index += 1;
        if cursor.is_none() {
            return Ok(content);
        }
    }
}

#[tokio::test]
async fn long_goal_and_strategy_survive_compaction_and_restart_exactly() -> Result<()> {
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
                name: "Long run state".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "long-run-state-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let body = |label: &str| format!("{label} clause “détail” \"quoted\".\n").repeat(205);
    let goal = format!(
        "GOAL-HEAD {} GOAL-MIDDLE {} GOAL-TAIL: never cite the draft schedule.",
        body("Review"),
        body("Check")
    );
    let strategy = format!(
        "STRATEGY-HEAD {} STRATEGY-MIDDLE {} STRATEGY-TAIL: verify the signed schedule first.",
        body("Compare"),
        body("Trace")
    );
    assert!(goal.len() > 15_000 && goal.len() <= 16 * 1024);
    assert!(strategy.len() > 15_000 && strategy.len() <= 16 * 1024);
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
                idempotency_key: "long-run-state-run".to_string(),
            },
        })
        .await?;
    responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "set-strategy",
                    "stateful_run_update",
                    &json!({
                        "expectedRevision": started.run.revision,
                        "status": "running",
                        "strategy": strategy,
                    })
                    .to_string(),
                ),
                responses::ev_completed("set-strategy-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("strategy-set", "Strategy recorded."),
                responses::ev_completed("strategy-set-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("summary", "COMPACTED_SUMMARY"),
                responses::ev_completed("summary-response"),
            ]),
        ],
    )
    .await;
    run_simple_turn(&mut server, &thread.thread.id).await?;
    let compact_id = server
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: thread.thread.id.clone(),
        })
        .await?;
    let _: ThreadCompactStartResponse = server.read_response(compact_id).await?;
    let _: TurnCompletedNotification = server.read_notification("turn/completed").await?;
    drop(server);

    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let resume_id = server
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread.thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = server.read_response(resume_id).await?;
    let attached: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(thread.thread.id.clone()),
            },
        })
        .await?;
    let attached = attached
        .run
        .expect("the resumed thread keeps its active run");
    assert_eq!(
        (attached.id.as_str(), attached.status),
        (started.run.id.as_str(), StatefulRunStatus::Running)
    );

    let paged_goal = read_every_page(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "goal",
        None,
    )
    .await?;
    let paged_strategy = read_every_page(
        &responses_server,
        &mut server,
        &thread.thread.id,
        "strategy",
        None,
    )
    .await?;
    assert_eq!((paged_goal, paged_strategy), (goal, strategy));
    Ok(())
}

#[cfg(not(target_os = "windows"))]
/// Probes the project fence until the in-flight completion demonstrably holds it: a
/// probe that still acquires the fence proves nothing yet, so it is released and
/// retried; a probe that times out does. Avoids a timing guess under load.
async fn wait_until_completion_holds_fence(blackboard: &BlackboardStore) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match tokio::time::timeout(
            Duration::from_millis(100),
            blackboard.acquire_completion_fence(Duration::from_millis(50)),
        )
        .await
        {
            Ok(Ok(probe)) => {
                probe.release().await?;
                anyhow::ensure!(
                    tokio::time::Instant::now() < deadline,
                    "completion never took the project fence"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok(Err(_)) | Err(_) => return Ok(()),
        }
    }
}
