use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::BlackboardQueryParams;
use codex_app_server_protocol::BlackboardQueryResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn model_receives_current_historical_selection_after_superseding_knowledge() -> Result<()> {
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
                name: "Blackboard lifecycle project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-blackboard-lifecycle-project".to_string(),
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
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;

    let record_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record-findings",
                    "blackboard_record_batch",
                    &json!({
                        "records": [
                            {
                                "idempotencyKey": "old-decision",
                                "kind": "decision",
                                "content": "The earlier decision.",
                                "confidenceBasisPoints": 9000,
                                "verification": "unverified",
                                "importance": "high",
                                "rootPromotion": "promoted"
                            },
                            {
                                "idempotencyKey": "new-decision",
                                "kind": "decision",
                                "content": "The replacement decision.",
                                "confidenceBasisPoints": 9500,
                                "verification": "unverified",
                                "importance": "high",
                                "rootPromotion": "promoted"
                            }
                        ]
                    })
                    .to_string(),
                ),
                responses::ev_completed("record-findings-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("record-done", "Findings recorded"),
                responses::ev_completed("record-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let record_output: serde_json::Value = serde_json::from_str(
        &record_log
            .function_call_output_text("record-findings")
            .expect("record output should be text"),
    )?;
    assert_eq!(record_output["failed"], 0, "{record_output}");
    let old_entry_id = record_output["results"][0]["entryId"]
        .as_str()
        .expect("old entry ID should be returned");
    let successor_entry_id = record_output["results"][1]["entryId"]
        .as_str()
        .expect("successor entry ID should be returned");

    let update_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "supersede-finding",
                    "blackboard_update_batch",
                    &json!({
                        "mutations": [{
                            "action": "supersede",
                            "entryId": old_entry_id,
                            "expectedRevision": 1,
                            "successorEntryId": successor_entry_id
                        }]
                    })
                    .to_string(),
                ),
                responses::ev_completed("supersede-finding-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("supersede-done", "Finding superseded"),
                responses::ev_completed("supersede-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let update_output: serde_json::Value = serde_json::from_str(
        &update_log
            .function_call_output_text("supersede-finding")
            .expect("update output should be text"),
    )?;
    assert_eq!(
        update_output["results"][0]["historicalFinding"],
        json!({
            "entryId": old_entry_id,
            "revision": 2,
        })
    );
    assert_eq!(update_output["results"][0]["state"], "superseded");
    Ok(())
}

#[tokio::test]
async fn model_cannot_self_award_user_confirmed_knowledge() -> Result<()> {
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
                name: "User confirmation boundary".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "user-confirmation-boundary".to_string(),
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
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;

    let record_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record-confirmation",
                    "blackboard_record_batch",
                    &json!({
                        "records": [
                            {
                                "idempotencyKey": "forged-confirmation",
                                "kind": "instruction",
                                "content": "The user did not confirm this instruction.",
                                "confidenceBasisPoints": 10_000,
                                "verification": "userConfirmed",
                                "importance": "critical",
                                "rootPromotion": "promoted"
                            },
                            {
                                "idempotencyKey": "agent-observation",
                                "kind": "note",
                                "content": "The model may still record ordinary knowledge.",
                                "confidenceBasisPoints": 8_000,
                                "verification": "unverified",
                                "importance": "normal",
                                "rootPromotion": "notPromoted"
                            }
                        ]
                    })
                    .to_string(),
                ),
                responses::ev_completed("record-confirmation-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("record-confirmation-done", "Boundary checked"),
                responses::ev_completed("record-confirmation-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let output: serde_json::Value = serde_json::from_str(
        &record_log
            .function_call_output_text("record-confirmation")
            .expect("record output should be text"),
    )?;
    assert_eq!(output["recorded"], 1);
    assert_eq!(output["failed"], 1);
    assert_eq!(output["results"][0]["recorded"], false);
    assert_eq!(
        output["results"][0]["error"],
        "userConfirmed is issued only from a host-observed user action and cannot be selected by the model"
    );
    assert_eq!(output["results"][1]["recorded"], true);

    let first_request = record_log.requests()[0].body_json();
    let batch_tool = first_request["tools"]
        .as_array()
        .expect("tools should be an array")
        .iter()
        .find(|tool| tool["name"] == "blackboard_record_batch")
        .expect("blackboard record batch tool should be available");
    assert_eq!(
        batch_tool["parameters"]["properties"]["records"]["items"]["properties"]["verification"]["enum"],
        json!(["unverified", "sourceVerified", "disputed", "stale"])
    );

    let knowledge: BlackboardQueryResponse = server
        .request(|request_id| ClientRequest::BlackboardQuery {
            request_id,
            params: BlackboardQueryParams {
                project_id: created.project.id,
                text: None,
                within_node_id: None,
                limit: Some(10),
            },
        })
        .await?;
    assert_eq!(knowledge.data.len(), 1);
    assert_eq!(
        knowledge.data[0].entry.content,
        "The model may still record ordinary knowledge."
    );
    Ok(())
}

#[tokio::test]
async fn model_can_enumerate_active_knowledge_affected_by_a_changed_route() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let source_path = project_root.path().join("facts.md");
    std::fs::write(&source_path, "# Authority\nThe approval threshold is 10.\n")?;
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
                name: "Changed authority project".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "stateful-changed-authority-project".to_string(),
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
    let started = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id),
            ..Default::default()
        })
        .await?;

    let route_query_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "route-query",
                    "context_map_query",
                    &json!({"text": "approval threshold"}).to_string(),
                ),
                responses::ev_completed("route-query-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("route-query-done", "Region located"),
                responses::ev_completed("route-query-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let route_query_output: serde_json::Value = serde_json::from_str(
        &route_query_log
            .function_call_output_text("route-query")
            .expect("route query output should be text"),
    )?;
    let region_route = route_query_output["data"]
        .as_array()
        .expect("route query should return data")
        .iter()
        .find_map(|item| {
            item["evidenceRoute"]["lineRange"]
                .is_object()
                .then(|| item["evidenceRoute"].clone())
        })
        .expect("query should return an exact region route");
    let region_context_map_entry_id = region_route["contextMapEntryId"]
        .as_str()
        .expect("region route should have an entry ID")
        .to_string();

    let initial_read_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "initial-read",
                    "evidence_read",
                    &json!({"evidenceRoute": region_route}).to_string(),
                ),
                responses::ev_completed("initial-read-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("initial-read-done", "Evidence reviewed"),
                responses::ev_completed("initial-read-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let initial_read_output: serde_json::Value = serde_json::from_str(
        &initial_read_log
            .function_call_output_text("initial-read")
            .expect("initial evidence output should be text"),
    )?;
    assert_eq!(
        initial_read_output["contextMapEntryId"],
        region_context_map_entry_id
    );
    let read_receipt_id = initial_read_output["blackboardEvidence"]["readReceiptId"]
        .as_str()
        .expect("complete evidence should include a read receipt");

    let record_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "record-authority",
                    "blackboard_record_batch",
                    &json!({
                        "records": [{
                            "idempotencyKey": "approval-threshold",
                            "kind": "fact",
                            "content": "The approval threshold is 10.",
                            "confidenceBasisPoints": 9800,
                            "verification": "sourceVerified",
                            "importance": "high",
                            "rootPromotion": "promoted",
                            "evidence": [{"readReceiptId": read_receipt_id}]
                        }]
                    })
                    .to_string(),
                ),
                responses::ev_completed("record-authority-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("record-authority-done", "Authority recorded"),
                responses::ev_completed("record-authority-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let record_output: serde_json::Value = serde_json::from_str(
        &record_log
            .function_call_output_text("record-authority")
            .expect("record output should be text"),
    )?;
    assert_eq!(record_output["failed"], 0, "{record_output}");
    let blackboard_entry_id = record_output["results"][0]["entryId"]
        .as_str()
        .expect("recorded knowledge should have an entry ID");

    std::fs::write(&source_path, "# Authority\nThe approval threshold is 6.\n")?;
    let refreshed_read_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "refreshed-read",
                    "evidence_read",
                    &json!({"relativePath": "facts.md"}).to_string(),
                ),
                responses::ev_completed("refreshed-read-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("refreshed-read-done", "Changed evidence reviewed"),
                responses::ev_completed("refreshed-read-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let refreshed_read_output: serde_json::Value = serde_json::from_str(
        &refreshed_read_log
            .function_call_output_text("refreshed-read")
            .expect("refreshed evidence output should be text"),
    )?;
    assert_eq!(refreshed_read_output["sourceRefreshed"], true);
    let refreshed_context_map_entry_id = refreshed_read_output["contextMapEntryId"]
        .as_str()
        .expect("refreshed source read should return its file route ID")
        .to_string();
    assert_ne!(refreshed_context_map_entry_id, region_context_map_entry_id);

    let dependent_query_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "query-dependents",
                    "blackboard_query",
                    &json!({
                        "evidenceContextMapEntryIds": [refreshed_context_map_entry_id],
                        "entryScope": "active",
                        "limit": 10
                    })
                    .to_string(),
                ),
                responses::ev_completed("query-dependents-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("query-dependents-done", "Dependents reviewed"),
                responses::ev_completed("query-dependents-done-response"),
            ]),
        ],
    )
    .await;
    run_turn(&mut server, &started.thread.id).await?;
    let dependent_query_output: serde_json::Value = serde_json::from_str(
        &dependent_query_log
            .function_call_output_text("query-dependents")
            .expect("dependent query output should be text"),
    )?;
    let node_id = dependent_query_output["data"][0]["nodeId"]
        .as_str()
        .expect("dependent knowledge should retain its hierarchy node");
    assert_eq!(
        dependent_query_output["data"],
        json!([{
            "entryId": blackboard_entry_id,
            "nodeId": node_id,
            "revision": 1,
            "state": "active",
            "supersededBy": null,
            "kind": "fact",
            "content": "The approval threshold is 10.",
            "structuredValue": null,
            "confidenceBasisPoints": 9800,
            "declaredVerification": "sourceVerified",
            "effectiveVerification": "stale",
            "evidenceFreshness": "stale",
            "storedEvidenceFreshness": "stale",
            "importance": "high",
            "rootPromotion": "promoted",
            "evidenceCount": 1,
            "provenance": {
                "kind": "agent",
                "sourceId": "record-authority"
            },
            "detailsOmitted": ["evidenceLocators", "relations"]
        }])
    );
    assert_eq!(dependent_query_output["truncated"], false);
    assert!(dependent_query_output["projectRevision"].as_u64().is_some());
    assert_eq!(
        dependent_query_output["nextAfterEntryId"],
        serde_json::Value::Null
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
