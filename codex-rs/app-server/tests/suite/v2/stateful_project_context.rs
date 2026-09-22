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
use codex_project_intelligence::ContextMapStore;
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
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
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
    seed_context_map(codex_home.path(), &created.project.id, project_root.path()).await?;
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
    assert!(requests[0].body_contains_text("smallest decisive source set"));
    let blackboard_output = requests[1]
        .function_call_output("blackboard-call")
        .to_string();
    assert!(blackboard_output.contains("A decisive project fact survives every thread view."));
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
    std::fs::write(
        project_root.path().join("decision.md"),
        "preamble\ndecisive clause\ncontrolling number: 42\nunrelated appendix\n",
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
    assert_eq!(
        output["content"],
        "decisive clause\ncontrolling number: 42\n"
    );
    assert_eq!(output["firstLine"], 2);
    assert_eq!(output["lastLine"], 3);
    assert_eq!(output["truncated"], false);
    assert_eq!(output["maxBytesApplied"], 12_288);
    assert_eq!(output["maxBytesClamped"], true);
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
    assert_eq!(output["filesIndexed"], 2);
    assert_eq!(output["routesTruncated"], false);
    assert_eq!(output["routes"].as_array().map(Vec::len), Some(2));
    assert_eq!(output["routes"][0]["source"]["relativePath"], "alpha.md");
    assert_eq!(output["routes"][1]["source"]["relativePath"], "beta.md");
    assert!(
        output["routes"][0]["description"]
            .as_str()
            .is_some_and(|description| description.contains("# Alpha"))
    );
    Ok(())
}

#[tokio::test]
async fn model_can_batch_record_and_retrieve_project_learning() -> Result<()> {
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
    seed_root_blackboard(codex_home.path(), &created.project.id).await?;
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
                                "verification": "unverified",
                                "importance": "high",
                                "rootPromotion": "promoted"
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
                    &json!({"text": "durable project state"}).to_string(),
                ),
                responses::ev_completed("query-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
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

    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].body_contains_text("blackboard_record_batch"));
    assert!(requests[0].body_contains_text("blackboard_relate"));
    assert!(requests[0].body_contains_text("context_map_refresh"));
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
    assert!(
        requests[2]
            .function_call_output("query-call")
            .to_string()
            .contains("Use the durable project state before rereading source files.")
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
    assert!(body.contains("Do not query deeper state, search by every known filename"));
    Ok(())
}

async fn seed_root_blackboard(codex_home: &std::path::Path, project_id: &str) -> Result<()> {
    let sqlite = SqliteConfig::new_for_testing(codex_home.abs());
    let hierarchy = HierarchyStore::open(&sqlite).await?;
    let project_node_id = HierarchyNodeId::parse(format!("project-node-{project_id}"))?;
    hierarchy
        .create_node(
            project_node_id.clone(),
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
    BlackboardStore::open(&sqlite)
        .await?
        .create_entry(
            BlackboardEntryId::parse(format!("project-fact-{project_id}"))?,
            NewBlackboardEntry {
                project_id: project_id.to_string(),
                node_id: project_node_id,
                kind: BlackboardKind::Fact,
                content: "A decisive project fact survives every thread view.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(8_500)?,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
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
) -> Result<()> {
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
    let source_fingerprint = SourceFingerprint::parse("sha256:readme-v1")?;
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
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id,
                    source_fingerprint,
                }],
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "integration-fixture".to_string(),
                },
            },
        )
        .await?;
    Ok(())
}
