use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
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
