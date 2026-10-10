use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::ContextMapCoverage;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapListQuery;
use codex_project_intelligence::ContextMapQuery;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::EvidenceRoute;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewContextMapEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SourceFingerprint;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;

#[tokio::test]
async fn selected_project_context_survives_fork_and_cold_resume() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Decisive Evidence Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-context-project".to_string(),
            },
        })
        .await?;
    seed_root_blackboard(codex_home.path(), &created.project.id).await?;
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;
    run_turn(&mut server, &started.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    let hierarchy =
        HierarchyStore::open(&SqliteConfig::new_for_testing(codex_home.path().abs())).await?;
    hierarchy
        .create_node(
            HierarchyNodeId::parse(format!("root-node-{}", created.project.id))?,
            NewHierarchyNode {
                project_id: created.project.id.clone(),
                parent_id: Some(HierarchyNodeId::parse(format!(
                    "project-node-{}",
                    created.project.id
                ))?),
                kind: NodeKind::Directory,
                project_root: Some(project_root.path().display().to_string()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    run_turn(&mut server, &started.thread.id).await?;
    let requests = responses.received_requests().await.unwrap_or_default();
    let body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("model request should be recorded")
        .body_json::<serde_json::Value>()?
        .to_string();
    assert!(body.contains("<stateful_project_update>"));
    assert!(
        body.contains("model-visible root blackboard knowledge and source routes are unchanged")
    );

    let compact_request = server
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: started.thread.id.clone(),
        })
        .await?;
    let _: ThreadCompactStartResponse = server.read_response(compact_request).await?;
    let _: codex_app_server_protocol::TurnCompletedNotification =
        server.read_notification("turn/completed").await?;
    run_turn(&mut server, &started.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    let forked: ThreadForkResponse = server
        .request(|request_id| ClientRequest::ThreadFork {
            request_id,
            params: ThreadForkParams {
                thread_id: started.thread.id.clone(),
                ..Default::default()
            },
        })
        .await?;
    run_turn(&mut server, &forked.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    drop(server);
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let resumed: ThreadResumeResponse = server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: started.thread.id.clone(),
                ..Default::default()
            },
        })
        .await?;
    run_turn(&mut server, &resumed.thread.id).await?;
    assert_latest_request_has_project(&responses, &created.project.id).await?;

    let unselected = server.start_thread(ThreadStartParams::default()).await?;
    run_turn(&mut server, &unselected.thread.id).await?;
    let requests = responses.received_requests().await.unwrap_or_default();
    let body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("model request should be recorded")
        .body_json::<serde_json::Value>()?
        .to_string();
    assert!(!body.contains("<stateful_project>"));
    Ok(())
}

#[tokio::test]
async fn incomplete_refresh_health_is_visible_in_the_next_model_request() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(project_root.path().join("README.md"), "# Indexed source\n")?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Incomplete refresh project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "incomplete-refresh-project".to_string(),
            },
        })
        .await?;
    server
        .request::<ContextMapRefreshResponse>(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let _hierarchy = HierarchyStore::open(&sqlite).await?;
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join("project_intelligence_1.sqlite"))
        .await?;
    sqlx::query(
        "UPDATE project_index_refresh_status
         SET inventory_complete = 0, files_skipped = 1, truncated = 1
         WHERE project_id = ?",
    )
    .bind(&created.project.id)
    .execute(&pool)
    .await?;
    pool.close().await;

    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;
    run_turn(&mut server, &started.thread.id).await?;
    let requests = responses.received_requests().await.unwrap_or_default();
    let body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("model request should be recorded")
        .body_json::<serde_json::Value>()?
        .to_string();

    assert!(body.contains("inventoryComplete=false"));
    assert!(body.contains("filesSkipped=1"));
    assert!(body.contains("Do not infer that an unindexed file is absent"));
    Ok(())
}

#[tokio::test]
async fn unchecked_user_confirmed_evidence_replaces_the_current_packet() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Freshness Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-freshness-project".to_string(),
            },
        })
        .await?;
    seed_root_blackboard(codex_home.path(), &created.project.id).await?;
    seed_context_map(
        codex_home.path(),
        &created.project.id,
        project_root.path(),
        BlackboardVerification::UserConfirmed,
    )
    .await?;
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;

    run_turn(&mut server, &started.thread.id).await?;
    let requests = responses.received_requests().await.unwrap_or_default();
    let initial_body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("initial model request should be recorded")
        .body_json::<serde_json::Value>()?
        .to_string();
    assert!(
        initial_body
            .contains("verification=userConfirmed; declared=userConfirmed; evidence=current")
    );
    assert!(initial_body.contains("README.md (current)"));

    std::fs::remove_file(project_root.path().join("README.md"))?;
    std::fs::create_dir(project_root.path().join("README.md"))?;
    run_turn(&mut server, &started.thread.id).await?;

    let requests = responses.received_requests().await.unwrap_or_default();
    let current_body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("current model request should be recorded")
        .body_json::<serde_json::Value>()?
        .to_string();
    assert!(current_body.contains(
        "verification=userConfirmed; declared=userConfirmed; evidence=uncheckedThisTurn"
    ));
    assert!(current_body.contains("README.md (uncheckedThisTurn)"));
    Ok(())
}

#[tokio::test]
async fn fresh_thread_receives_the_exact_earlier_conversation_once() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Cross-thread continuity".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "cross-thread-continuity-project".to_string(),
            },
        })
        .await?;
    let first = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: first.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "FIRST_THREAD_PRIVATE_TRANSCRIPT_MARKER".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let store = StatefulRunStore::open(&sqlite).await?;
    let run_id = StatefulRunId::parse("cross-thread-completed-run")?;
    let run = store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: created.project.id.clone(),
                thread_ids: vec![first.thread.id.clone()],
                goal: "Determine the deployment gate.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await?;
    complete_seeded_run(
        &store,
        &run_id,
        StatefulRunUpdate {
            expected_revision: run.revision,
            status: StatefulRunStatus::Completed,
            strategy: Some("Reuse the verified checksum decision.".to_string()),
            result: Some(
                "The deployment gate is green only after checksum verification.".to_string(),
            ),
        },
        Some((
            "cross-thread-final-obligation".to_string(),
            NewObligation {
                project_id: created.project.id.clone(),
                run_id: run_id.clone(),
                packet: ObligationPacket {
                    learning: vec![
                        "Checksum verification is the decisive deployment condition.".to_string(),
                    ],
                    implication: vec![
                        "Later work should start from the verified deployment gate.".to_string(),
                    ],
                    ..Default::default()
                },
                provenance_source_id: "first-thread-completion".to_string(),
            },
        )),
    )
    .await?;

    let second = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;
    run_turn(&mut server, &second.thread.id).await?;
    run_turn(&mut server, &second.thread.id).await?;
    let requests = responses.received_requests().await.unwrap_or_default();
    let bodies = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(|request| {
            request
                .body_json::<serde_json::Value>()
                .map(|body| body.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [.., fresh, follow_up] = bodies.as_slice() else {
        panic!("both fresh-thread model requests should be recorded");
    };
    // The earlier thread's exact request and final answer reach the fresh thread.
    assert!(fresh.contains("<stateful_continuity>"));
    assert!(fresh.contains("FIRST_THREAD_PRIVATE_TRANSCRIPT_MARKER"));
    assert!(fresh.contains(&first.thread.id));
    assert!(fresh.contains("Answer: \\\"Done\\\""));
    assert!(!fresh.contains("<stateful_project_outcomes>"));
    // The record is rendered once per context window: the next request holds the same single
    // record in its history instead of a second insertion.
    assert_eq!(follow_up.matches("<stateful_continuity>").count(), 1);
    // A fresh window's Stateful developer content stays within the 12 KiB window budget.
    let fresh_request = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .nth_back(1)
        .expect("fresh request")
        .body_json::<serde_json::Value>()?;
    let stateful_bytes = fresh_request["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["role"] == "developer")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
        .filter(|text| text.trim_start().starts_with("<stateful"))
        .map(str::len)
        .sum::<usize>();
    assert!(stateful_bytes <= 12 * 1024, "{stateful_bytes} bytes");
    Ok(())
}

/// The latest-run line follows the recalled threads: a run whose bound turn is in an
/// archived thread no longer contributes its strategy to a fresh thread.
#[tokio::test]
async fn archiving_a_thread_removes_its_run_from_recall() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Archived recall".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "archived-recall-project".to_string(),
            },
        })
        .await?;
    let project_id = created.project.id;
    let worked = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let store =
        StatefulRunStore::open(&SqliteConfig::new_for_testing(codex_home.path().abs())).await?;
    let run_id = StatefulRunId::parse("archived-recall-run")?;
    let run = store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: project_id.clone(),
                thread_ids: vec![worked.clone()],
                goal: "Plan the migration.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await?;
    store
        .update_run(
            &run_id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Running,
                strategy: Some("ARCHIVED_STRATEGY_MARKER migrate the schema first.".to_string()),
                result: None,
            },
        )
        .await?;
    run_turn(&mut server, &worked).await?;

    let fresh_body = |server_requests: &[wiremock::Request]| -> Result<String> {
        Ok(server_requests
            .iter()
            .rev()
            .find(|request| request.url.path().ends_with("/responses"))
            .expect("a model request should be recorded")
            .body_json::<serde_json::Value>()?
            .to_string())
    };
    let before = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut server, &before).await?;
    let body = fresh_body(&responses.received_requests().await.unwrap_or_default())?;
    assert!(body.contains("ARCHIVED_STRATEGY_MARKER"));
    assert!(body.contains("archived-recall-run"));

    let _: codex_app_server_protocol::ThreadArchiveResponse = server
        .request(|request_id| ClientRequest::ThreadArchive {
            request_id,
            params: codex_app_server_protocol::ThreadArchiveParams {
                thread_id: worked.clone(),
            },
        })
        .await?;
    let after = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut server, &after).await?;
    let body = fresh_body(&responses.received_requests().await.unwrap_or_default())?;
    assert!(!body.contains("ARCHIVED_STRATEGY_MARKER"));
    assert!(!body.contains(&worked));
    Ok(())
}

#[tokio::test]
async fn project_intelligence_tools_query_shared_state_and_exact_sources() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("blackboard-response"),
                responses::ev_function_call(
                    "blackboard-call",
                    "blackboard_query",
                    &json!({"text": "decisive"}).to_string(),
                ),
                responses::ev_completed("blackboard-response"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("context-map-response"),
                responses::ev_function_call(
                    "context-map-call",
                    "context_map_query",
                    &json!({"text": "operator setup"}).to_string(),
                ),
                responses::ev_completed("context-map-response"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("done-response"),
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Decisive Evidence Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-query-tools-project".to_string(),
            },
        })
        .await?;
    seed_root_blackboard(codex_home.path(), &created.project.id).await?;
    seed_context_map(
        codex_home.path(),
        &created.project.id,
        project_root.path(),
        BlackboardVerification::SourceVerified,
    )
    .await?;
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;

    run_turn(&mut server, &started.thread.id).await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].body_contains_text("blackboard_query"));
    assert!(requests[0].body_contains_text("context_map_query"));
    assert!(requests[0].body_contains_text("blackboard_record_batch"));
    assert!(requests[0].body_contains_text("README.md (current)"));
    assert!(requests[0].body_contains_text("check only what the task depends on"));
    let blackboard_output = requests[1]
        .function_call_output("blackboard-call")
        .to_string();
    assert!(requests[0].body_contains_text("A decisive project fact survives every thread view."));
    // E2: the root lists the user-stated README fact first.
    assert!(
        blackboard_output.contains(r#"\"rootAlias\":\"E2\""#),
        "{blackboard_output}"
    );
    assert!(blackboard_output.contains(r#"\"contentInRoot\":true"#));
    assert!(!blackboard_output.contains("A decisive project fact survives every thread view."));
    assert!(blackboard_output.contains(&created.project.id));
    let context_map_output = requests[2]
        .function_call_output("context-map-call")
        .to_string();
    assert!(context_map_output.contains("Project purpose, setup, and operator instructions."));
    assert!(context_map_output.contains("README.md"));
    assert!(context_map_output.contains("current"));
    Ok(())
}

#[tokio::test]
async fn model_reads_only_a_fingerprint_verified_source_region() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "evidence-call",
                    "evidence_read",
                    &json!({
                        "relativePath": "decision.md",
                        "lineRange": {"start": 2, "end": 3},
                        "maxBytes": 100_000
                    })
                    .to_string(),
                ),
                responses::ev_completed("evidence-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let source = "preamble\ndecisive clause\ncontrolling number: 42\nunrelated appendix\n";
    std::fs::write(project_root.path().join("decision.md"), source)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Selective evidence project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-selective-evidence-project".to_string(),
            },
        })
        .await?;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    let project_id = created.project.id.clone();
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;

    run_turn(&mut server, &started.thread.id).await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].body_contains_text("evidence_read"));
    let output: serde_json::Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("evidence-call")
            .expect("evidence output should be text"),
    )?;
    let receipt_id = output["blackboardEvidence"]["readReceiptId"]
        .as_str()
        .expect("a complete read carries a receipt");
    assert!(receipt_id.starts_with("stateful-read-"));
    assert_eq!(
        output,
        json!({
            "projectId": project_id,
            "contextMapEntryId": output["contextMapEntryId"],
            "sourceFingerprint": format!("sha256:{:x}", Sha256::digest(source)),
            "source": {
                "projectRoot": output["source"]["projectRoot"],
                "relativePath": "decision.md",
            },
            "contentFormat": "lineNumbered",
            "content": "L2: decisive clause\nL3: controlling number: 42\n",
            "bytesReturned": "decisive clause\ncontrolling number: 42\n".len(),
            "totalBytes": source.len(),
            "totalLines": 4,
            "firstLine": 2,
            "lastLine": 3,
            "lastLinePartial": false,
            "truncated": false,
            "maxBytesApplied": 12_288,
            "maxBytesClamped": true,
            "sourceRefreshed": false,
            "blackboardEvidence": {"readReceiptId": receipt_id},
            "revision": output["revision"],
        })
    );
    Ok(())
}

/// Both cuts end between labelled lines: a small maxBytes cuts the source read, and a
/// read larger than the response budget is cut while packing. Neither may carry a
/// receipt, and every counter must describe exactly the emitted lines.
#[tokio::test]
async fn model_truncated_evidence_reads_cut_between_lines_without_receipts() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "source-cut",
                    "evidence_read",
                    &json!({"relativePath": "ledger.txt", "maxBytes": 70}).to_string(),
                ),
                responses::ev_completed("source-cut-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "response-cut",
                    "evidence_read",
                    &json!({"relativePath": "ledger.txt", "maxBytes": 12_288}).to_string(),
                ),
                responses::ev_completed("response-cut-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let lines = (1..=400)
        .map(|line| format!("line {line:03} {}\n", "x".repeat(20)))
        .collect::<Vec<_>>();
    std::fs::write(project_root.path().join("ledger.txt"), lines.concat())?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Truncated evidence project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-truncated-evidence-project".to_string(),
            },
        })
        .await?;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;

    run_turn(&mut server, &started.thread.id).await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    let labelled = |first: usize, last: usize| {
        (first..=last)
            .map(|line| format!("L{line}: {}", lines[line - 1]))
            .collect::<String>()
    };
    let source_bytes = |first: usize, last: usize| lines[first - 1..last].concat().len();
    let summary = |output: &serde_json::Value| {
        json!({
            "content": output["content"],
            "bytesReturned": output["bytesReturned"],
            "firstLine": output["firstLine"],
            "lastLine": output["lastLine"],
            "lastLinePartial": output["lastLinePartial"],
            "truncated": output["truncated"],
            "blackboardEvidence": output["blackboardEvidence"],
        })
    };

    let source_cut_text = requests[1]
        .function_call_output_text("source-cut")
        .expect("source-cut output should be text");
    let source_cut: serde_json::Value = serde_json::from_str(&source_cut_text)?;
    assert_eq!(
        summary(&source_cut),
        json!({
            "content": labelled(1, 2),
            "bytesReturned": source_bytes(1, 2),
            "firstLine": 1,
            "lastLine": 2,
            "lastLinePartial": false,
            "truncated": true,
            "blackboardEvidence": null,
        })
    );

    let response_cut_text = requests[2]
        .function_call_output_text("response-cut")
        .expect("response-cut output should be text");
    assert!(response_cut_text.len() <= 9_000);
    let response_cut: serde_json::Value = serde_json::from_str(&response_cut_text)?;
    let last_line = response_cut["lastLine"]
        .as_u64()
        .and_then(|line| usize::try_from(line).ok())
        .expect("a response-budget cut still returns whole lines");
    assert!(last_line < 400);
    assert_eq!(
        summary(&response_cut),
        json!({
            "content": labelled(1, last_line),
            "bytesReturned": source_bytes(1, last_line),
            "firstLine": 1,
            "lastLine": last_line,
            "lastLinePartial": false,
            "truncated": true,
            "blackboardEvidence": null,
        })
    );
    Ok(())
}

#[tokio::test]
async fn model_guarded_route_rejects_shifted_source_until_requeried() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let source_path = project_root.path().join("facts.md");
    let source = (1..=70)
        .map(|line| {
            if line == 70 {
                "decisive_route_fact".to_string()
            } else {
                format!("line {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&source_path, &source)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Guarded region project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-guarded-region-project".to_string(),
            },
        })
        .await?;
    let project_id = created.project.id;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: project_id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?;

    let first_query = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "first-query",
                    "context_map_query",
                    &json!({"text": "decisive_route_fact"}).to_string(),
                ),
                responses::ev_completed("first-query-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("first-query-done", "Route found"),
                responses::ev_completed("first-query-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let first_output: serde_json::Value = serde_json::from_str(
        &first_query
            .function_call_output_text("first-query")
            .expect("context output should be text"),
    )?;
    let stale_route = first_output["data"][0]["evidenceRoute"].clone();
    assert_eq!(stale_route["lineRange"], json!({"start": 65, "end": 70}));

    std::fs::write(&source_path, format!("inserted\n{source}"))?;
    let stale_query = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "stale-query",
                    "context_map_query",
                    &json!({"text": "decisive_route_fact"}).to_string(),
                ),
                responses::ev_completed("stale-query-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("stale-query-done", "Stale route found"),
                responses::ev_completed("stale-query-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let stale_query_output: serde_json::Value = serde_json::from_str(
        &stale_query
            .function_call_output_text("stale-query")
            .expect("stale context output should be text"),
    )?;
    assert_eq!(stale_query_output["data"][0]["freshness"], "stale");
    assert_eq!(
        stale_query_output["data"][0]["refreshInput"],
        json!({"relativePath": "facts.md"})
    );
    assert!(stale_query_output["data"][0].get("evidenceRoute").is_none());

    let stale_read = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "stale-read",
                    "evidence_read",
                    &json!({"evidenceRoute": stale_route}).to_string(),
                ),
                responses::ev_completed("stale-read-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("stale-read-done", "Route was stale"),
                responses::ev_completed("stale-read-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let stale_output = stale_read
        .function_call_output_text("stale-read")
        .expect("stale read output should be text");
    assert!(stale_output.contains("source changed after indexing"));
    assert!(!stale_output.contains("stateful-read-"));

    server
        .request::<ContextMapRefreshResponse>(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: project_id.clone(),
            },
        })
        .await?;
    let current_query = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "current-query",
                    "context_map_query",
                    &json!({"text": "decisive_route_fact"}).to_string(),
                ),
                responses::ev_completed("current-query-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("current-query-done", "Current route found"),
                responses::ev_completed("current-query-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let current_output: serde_json::Value = serde_json::from_str(
        &current_query
            .function_call_output_text("current-query")
            .expect("current context output should be text"),
    )?;
    let current_route = current_output["data"][0]["evidenceRoute"].clone();
    assert_eq!(current_route["lineRange"], json!({"start": 65, "end": 71}));

    let current_read = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "current-read",
                    "evidence_read",
                    &json!({"evidenceRoute": current_route}).to_string(),
                ),
                responses::ev_completed("current-read-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("current-read-done", "Evidence verified"),
                responses::ev_completed("current-read-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let read_output: serde_json::Value = serde_json::from_str(
        &current_read
            .function_call_output_text("current-read")
            .expect("current read output should be text"),
    )?;
    assert_eq!(read_output["lastLine"], 71);
    assert_eq!(read_output["truncated"], false);
    assert!(
        read_output["blackboardEvidence"]["readReceiptId"]
            .as_str()
            .is_some_and(|receipt_id| receipt_id.starts_with("stateful-read-"))
    );
    Ok(())
}

#[tokio::test]
async fn context_refresh_returns_bounded_source_routes_to_the_model() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call("refresh-call", "context_map_refresh", "{}"),
                responses::ev_completed("refresh-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("alpha.md"),
        "# Alpha\nfirst route\n",
    )?;
    std::fs::write(
        project_root.path().join("beta.md"),
        "# Beta\nsecond route\n",
    )?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Refresh routes project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-refresh-routes-project".to_string(),
            },
        })
        .await?;
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;

    run_turn(&mut server, &started.thread.id).await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    let output: serde_json::Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("refresh-call")
            .expect("refresh output should be text"),
    )?;
    assert_eq!(output["inventoryComplete"], true);
    assert_eq!(output["regionCoverageComplete"], true);
    assert_eq!(output["filesIndexed"], 2);
    assert_eq!(output["regionsIndexed"], 2);
    assert!(output["scanDurationMs"].as_u64().is_some());
    assert!(output["publicationDurationMs"].as_u64().is_some());
    assert_eq!(output["routesTruncated"], false);
    assert_eq!(output["knowledgeCoverageAvailable"], true);
    assert_eq!(output["routes"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        output["routes"][0]["headline"],
        "alpha.md | # Alpha | first route"
    );
    assert_eq!(
        output["routes"][1]["headline"],
        "beta.md | # Beta | second route"
    );
    for route in output["routes"]
        .as_array()
        .expect("routes should be an array")
    {
        assert_eq!(route["coverage"], "complete");
        assert_eq!(route["freshness"], "current");
        assert!(
            route["evidenceRoute"]["sourceFingerprint"]
                .as_str()
                .is_some_and(|fingerprint| fingerprint.starts_with("sha256:"))
        );
        assert!(route["evidenceRoute"]["lineRange"].is_null());
    }
    Ok(())
}

#[tokio::test]
async fn model_can_reuse_file_learning_from_a_child_region_without_rereading() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let mut source = (1..=127)
        .map(|line| format!("Background line {line}.\n"))
        .collect::<String>();
    source.push_str("Durable project state should route back to this exact region, and the exact source version must remain bound to the learned conclusion.\n");
    std::fs::write(project_root.path().join("decision.md"), source)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Learning Project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-model-learning-project".to_string(),
            },
        })
        .await?;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    let context_store =
        ContextMapStore::open(&SqliteConfig::new_for_testing(codex_home.path().abs())).await?;
    let file_route = context_store
        .list_project(ContextMapListQuery {
            project_id: created.project.id.clone(),
            max_results: 10,
        })
        .await?
        .into_iter()
        .next()
        .expect("refreshed file route should exist");
    let child_region = context_store
        .query(ContextMapQuery {
            project_id: created.project.id.clone(),
            text: "durable project state exact region".to_string(),
            max_results: 10,
        })
        .await?
        .data
        .into_iter()
        .find(|hit| hit.source.region_anchor.is_some())
        .expect("refreshed region route should exist");
    let evidence_route = EvidenceRoute::from_hit(&file_route)?;
    let evidence_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "evidence-call",
                    "evidence_read",
                    &json!({"evidenceRoute": evidence_route}).to_string(),
                ),
                responses::ev_completed("evidence-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("evidence-done-message", "Evidence reviewed"),
                responses::ev_completed("evidence-done-response"),
            ]),
        ],
    )
    .await;
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;
    run_turn(&mut server, &started.thread.id).await?;
    let evidence_output: serde_json::Value = serde_json::from_str(
        &evidence_log
            .function_call_output_text("evidence-call")
            .expect("evidence output should be text"),
    )?;
    assert_eq!(
        evidence_output["contextMapEntryId"],
        file_route.entry.id.to_string()
    );
    let read_receipt_id = evidence_output["blackboardEvidence"]["readReceiptId"]
        .as_str()
        .expect("complete evidence read should return a receipt")
        .to_string();
    let first_line = evidence_output["firstLine"]
        .as_u64()
        .expect("region read should have a first line");
    let last_line = evidence_output["lastLine"]
        .as_u64()
        .expect("region read should have a last line");
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record-call",
                    "blackboard_record_batch",
                    &json!({
                        "records": [
                            {
                                "idempotencyKey": "strategy-learned",
                                "kind": "strategy",
                                "content": "Use the durable project state before rereading source files.",
                                "confidenceBasisPoints": 8000,
                                "verification": "sourceVerified",
                                "importance": "high",
                                "rootPromotion": "promoted",
                                "evidence": [{
                                    "readReceiptId": read_receipt_id
                                }]
                            },
                            {
                                "idempotencyKey": "open-question-learned",
                                "kind": "question",
                                "content": "Which exact source can change the current strategy?",
                                "confidenceBasisPoints": 7000,
                                "verification": "unverified",
                                "importance": "normal",
                                "rootPromotion": "candidate"
                            }
                        ],
                        "relations": [
                            {
                                "idempotencyKey": "strategy-opens-question",
                                "fromRecordKey": "strategy-learned",
                                "toRecordKey": "open-question-learned",
                                "kind": "relatedTo",
                                "note": "The durable-state strategy raises the decisive-source question.",
                                "confidenceBasisPoints": 9000
                            }
                        ]
                    })
                    .to_string(),
                ),
                responses::ev_completed("record-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "query-call",
                    "blackboard_query",
                    &json!({"text": "durable project state", "detail": "full"}).to_string(),
                ),
                responses::ev_completed("query-response"),
            ]),
            responses::sse(vec![
                responses::ev_function_call(
                    "context-query-call",
                    "context_map_query",
                    &json!({"text": "durable project state exact region"}).to_string(),
                ),
                responses::ev_completed("context-query-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].body_contains_text("blackboard_record_batch"));
    assert!(requests[0].body_contains_text("readReceiptId"));
    let batch_output: serde_json::Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("record-call")
            .expect("batch output should be text"),
    )?;
    assert_eq!(batch_output["recorded"], 2);
    assert_eq!(batch_output["failed"], 0);
    assert_eq!(batch_output["relationsRecorded"], 1);
    assert_eq!(batch_output["relationsFailed"], 0);
    assert_eq!(batch_output["relationResults"][0]["recorded"], true);
    assert!(requests[1].body_contains_text(&format!("S1:L{first_line}-L{last_line}")));
    let query_output: serde_json::Value = serde_json::from_str(
        &requests[2]
            .function_call_output_text("query-call")
            .expect("query output should be text"),
    )?;
    assert_eq!(
        query_output["data"][0]["nodeId"],
        file_route.entry.value.node_id.to_string()
    );
    assert_eq!(
        query_output["data"][0]["declaredVerification"],
        "sourceVerified"
    );
    assert_eq!(
        query_output["data"][0]["effectiveVerification"],
        "sourceVerified"
    );
    assert_eq!(
        query_output["data"][0]["evidence"][0],
        json!({
            "contextMapEntryId": file_route.entry.id.to_string(),
            "sourceFingerprint": file_route.entry.value.source_fingerprint.to_string(),
            "lineRange": {"start": first_line, "end": last_line},
        })
    );
    let context_output: serde_json::Value = serde_json::from_str(
        &requests[3]
            .function_call_output_text("context-query-call")
            .expect("context-map output should be text"),
    )?;
    assert_eq!(context_output["knowledgeCoverageAvailable"], true);
    assert_eq!(context_output["mayHaveMore"], false);
    let headline = context_output["data"][0]["headline"]
        .as_str()
        .expect("query route should include a headline");
    assert!(headline.len() <= 240);
    assert!(headline.contains("Durable project state"));
    assert_eq!(
        context_output["data"][0]["evidenceRoute"]["contextMapEntryId"],
        child_region.entry.id.to_string()
    );
    assert_ne!(child_region.entry.id, file_route.entry.id);
    assert_eq!(
        context_output["data"][0]["knownKnowledge"],
        json!({"rootEntries": 1, "deeperEntries": 0})
    );
    Ok(())
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: "Continue the project work.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

async fn assert_latest_request_has_project(
    responses: &wiremock::MockServer,
    project_id: &str,
) -> Result<()> {
    let requests = responses.received_requests().await.unwrap_or_default();
    let body = requests
        .iter()
        .rev()
        .find(|request| request.url.path().ends_with("/responses"))
        .expect("model request should be recorded")
        .body_json::<serde_json::Value>()?;
    let body = body.to_string();
    assert!(body.contains("<stateful_project>"));
    assert!(body.contains(&format!("Project ID: {project_id}")));
    assert!(body.contains("Decisive Evidence Project"));
    assert!(body.contains("A decisive project fact survives every thread view."));
    assert!(body.contains("verification=unverified"));
    assert!(body.contains("continue from it instead of rediscovering it"));
    Ok(())
}

pub(super) async fn seed_root_blackboard(
    codex_home: &std::path::Path,
    project_id: &str,
) -> Result<()> {
    HierarchyStore::open(&SqliteConfig::new_for_testing(codex_home.abs()))
        .await?
        .create_node(
            HierarchyNodeId::parse(format!("project-node-{project_id}"))?,
            NewHierarchyNode {
                project_id: project_id.to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    promote_root_fact(
        codex_home,
        project_id,
        &format!("project-fact-{project_id}"),
        "A decisive project fact survives every thread view.",
    )
    .await
}

/// Adds a promoted, unverified root fact to a project seeded by `seed_root_blackboard`.
pub(super) async fn promote_root_fact(
    codex_home: &std::path::Path,
    project_id: &str,
    entry_id: &str,
    content: &str,
) -> Result<()> {
    BlackboardStore::open(&SqliteConfig::new_for_testing(codex_home.abs()))
        .await?
        .create_entry(
            BlackboardEntryId::parse(entry_id)?,
            NewBlackboardEntry {
                project_id: project_id.to_string(),
                node_id: HierarchyNodeId::parse(format!("project-node-{project_id}"))?,
                kind: BlackboardKind::Fact,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(8_500)?,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                // An agent finding: model tools never return the user's own memory.
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "integration-fixture".to_string(),
                },
            },
        )
        .await?;
    Ok(())
}

async fn seed_context_map(
    codex_home: &std::path::Path,
    project_id: &str,
    project_root: &std::path::Path,
    verification: BlackboardVerification,
) -> Result<()> {
    let source = b"# Project\n\nOperator setup and project instructions.\n";
    std::fs::write(project_root.join("README.md"), source)?;
    let sqlite = SqliteConfig::new_for_testing(codex_home.abs());
    let hierarchy = HierarchyStore::open(&sqlite).await?;
    let project_node_id = HierarchyNodeId::parse(format!("project-node-{project_id}"))?;
    let root_node_id = HierarchyNodeId::parse(format!("root-node-{project_id}"))?;
    let file_node_id = HierarchyNodeId::parse(format!("readme-node-{project_id}"))?;
    let project_root = project_root.display().to_string();
    hierarchy
        .create_node(
            root_node_id.clone(),
            NewHierarchyNode {
                project_id: project_id.to_string(),
                parent_id: Some(project_node_id),
                kind: NodeKind::Directory,
                project_root: Some(project_root.clone()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    let source_fingerprint =
        SourceFingerprint::parse(format!("sha256:{:x}", Sha256::digest(source)))?;
    hierarchy
        .create_node(
            file_node_id.clone(),
            NewHierarchyNode {
                project_id: project_id.to_string(),
                parent_id: Some(root_node_id),
                kind: NodeKind::File,
                project_root: Some(project_root),
                relative_path: ProjectRelativePath::parse("README.md")?,
                region_anchor: None,
                source_fingerprint: Some(source_fingerprint.clone()),
            },
        )
        .await?;
    let context_map_entry_id = ContextMapEntryId::parse(format!("readme-map-{project_id}"))?;
    ContextMapStore::open(&sqlite)
        .await?
        .create_entry(
            context_map_entry_id.clone(),
            NewContextMapEntry {
                project_id: project_id.to_string(),
                node_id: file_node_id.clone(),
                source_fingerprint: source_fingerprint.clone(),
                description: "Project purpose, setup, and operator instructions.".to_string(),
                routing_terms: vec!["operator".to_string(), "setup".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await?;
    BlackboardStore::open(&sqlite)
        .await?
        .create_entry(
            BlackboardEntryId::parse(format!("readme-fact-{project_id}"))?,
            NewBlackboardEntry {
                project_id: project_id.to_string(),
                node_id: file_node_id,
                kind: BlackboardKind::Fact,
                content: "README contains the current operator instructions.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(9_000)?,
                verification,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id,
                    source_fingerprint,
                    line_range: None,
                }],
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "integration-fixture".to_string(),
                },
            },
        )
        .await?;
    Ok(())
}

/// Seeds a completed run through the host completion decision, as the product does.
async fn complete_seeded_run(
    store: &codex_stateful_runtime::StatefulRunStore,
    run_id: &codex_stateful_runtime::StatefulRunId,
    update: StatefulRunUpdate,
    obligation: Option<(String, NewObligation)>,
) -> Result<()> {
    let latest = store.latest_obligation(run_id).await?;
    let commit = super::stateful_acceptance_support::seeded_commit(
        store,
        run_id,
        "seed",
        latest.map(|obligation| obligation.sequence),
    )
    .await?;
    store
        .complete_run_with_acceptance(run_id, update, &commit, obligation)
        .await?;
    Ok(())
}
