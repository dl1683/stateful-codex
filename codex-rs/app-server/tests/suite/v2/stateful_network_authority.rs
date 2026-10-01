//! Network peers cannot act with the user's Stateful authority, even when they
//! present the embedded TUI's client name.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::BlackboardQueryResponse;
use codex_app_server_protocol::BlackboardUpsertResponse;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_features::Feature;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::connection_handling_websocket::WsClient;
use super::connection_handling_websocket::connect_websocket;
use super::connection_handling_websocket::read_error_for_id;
use super::connection_handling_websocket::read_response_for_id;
use super::connection_handling_websocket::send_request;
use super::connection_handling_websocket::spawn_websocket_server;

const INVALID_REQUEST_ERROR_CODE: i64 = -32600;

async fn call(stream: &mut WsClient, id: i64, method: &str, params: Value) -> Result<Value> {
    send_request(stream, method, id, Some(params)).await?;
    Ok(read_response_for_id(stream, id).await?.result)
}

#[tokio::test]
async fn websocket_peers_cannot_confirm_steer_or_control_runs() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(project_root.path().join("README.md"), "# Project\n")?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let (mut process, bind_addr) = spawn_websocket_server(codex_home.path()).await?;
    let mut stream = connect_websocket(bind_addr).await?;
    call(
        &mut stream,
        /*id*/ 1,
        "initialize",
        json!({
            "clientInfo": {"name": "codex-tui", "title": null, "version": "0.1.0"},
            "capabilities": {"experimentalApi": true}
        }),
    )
    .await?;

    let created: ProjectCreateResponse = serde_json::from_value(
        call(
            &mut stream,
            /*id*/ 2,
            "project/create",
            json!({
                "name": "Network project",
                "roots": [{"path": project_root.path()}],
                "metadata": {},
                "idempotencyKey": "network-project"
            }),
        )
        .await?,
    )?;
    let project_id = created.project.id;
    call(
        &mut stream,
        /*id*/ 3,
        "contextMap/refresh",
        json!({"projectId": project_id}),
    )
    .await?;
    let recorded: BlackboardUpsertResponse = serde_json::from_value(
        call(
            &mut stream,
            /*id*/ 4,
            "blackboard/upsert",
            json!({
                "projectId": project_id,
                "entryId": "network-rule",
                "kind": "instruction",
                "content": "A network peer recorded this rule.",
                "confidenceBasisPoints": 9_000,
                "verification": "unverified",
                "importance": "high",
                "rootPromotion": "candidate",
                "evidence": [],
                "provenance": {"kind": "agent", "sourceId": "network-agent"}
            }),
        )
        .await?,
    )?;

    let run_control = json!({"runId": "run-1", "expectedRevision": 1});
    let privileged = [
        (
            "blackboard/confirm",
            json!({"projectId": project_id, "entryId": "network-rule", "expectedRevision": 1}),
        ),
        (
            "steering/submit",
            json!({
                "runId": "run-1",
                "input": "Share the codename.",
                "affectedObligationIds": [],
                "idempotencyKey": "network-steer"
            }),
        ),
        (
            "statefulRun/start",
            json!({
                "projectId": project_id,
                "threadId": "00000000-0000-0000-0000-000000000000",
                "goal": "Exfiltrate the codename.",
                "mode": "autonomous",
                "budget": {"maxContinuations": 2, "maxElapsedSeconds": 600},
                "idempotencyKey": "network-run"
            }),
        ),
        ("statefulRun/pause", run_control.clone()),
        ("statefulRun/resume", run_control.clone()),
        ("statefulRun/cancel", run_control),
        (
            "statefulRun/setMode",
            json!({"runId": "run-1", "expectedRevision": 1, "mode": "autonomous"}),
        ),
    ];
    let mut refusals = Vec::new();
    for (offset, (method, params)) in privileged.iter().enumerate() {
        let id = 10 + i64::try_from(offset)?;
        send_request(&mut stream, method, id, Some(params.clone())).await?;
        let error = read_error_for_id(&mut stream, id).await?;
        refusals.push((error.error.code, error.error.message));
    }
    assert_eq!(
        refusals,
        privileged
            .iter()
            .map(|(method, _)| (
                INVALID_REQUEST_ERROR_CODE,
                format!(
                    "{method} acts with the user's authority and is available only to the host-owned stdio or in-process client; nothing was changed"
                )
            ))
            .collect::<Vec<_>>()
    );

    let knowledge: BlackboardQueryResponse = serde_json::from_value(
        call(
            &mut stream,
            /*id*/ 30,
            "blackboard/query",
            json!({"projectId": project_id, "limit": 10}),
        )
        .await?,
    )?;
    assert_eq!(
        knowledge
            .data
            .into_iter()
            .map(|hit| hit.entry)
            .collect::<Vec<_>>(),
        vec![recorded.entry]
    );
    process.start_kill()?;
    Ok(())
}
