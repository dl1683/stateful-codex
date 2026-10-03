use codex_app_server_protocol::ClientRequest;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::require_user_authority;
use crate::outgoing_message::OutgoingMessage;
use crate::transport::ConnectionOrigin;
use crate::user_verification::test_support::Harness;

fn request(method: &str, params: Value) -> ClientRequest {
    serde_json::from_value(json!({"id": 1, "method": method, "params": params}))
        .expect("valid client request")
}

fn user_authority_requests() -> Vec<(&'static str, ClientRequest)> {
    let run_control = json!({"runId": "run-1", "expectedRevision": 1});
    vec![
        (
            "blackboard/confirm",
            request(
                "blackboard/confirm",
                json!({"projectId": "project-1", "entryId": "entry-1", "expectedRevision": 1}),
            ),
        ),
        (
            "steering/submit",
            request(
                "steering/submit",
                json!({
                    "runId": "run-1",
                    "input": "Only touch the parser.",
                    "affectedObligationIds": [],
                    "idempotencyKey": "steer-1"
                }),
            ),
        ),
        (
            "statefulRun/start",
            request(
                "statefulRun/start",
                json!({
                    "projectId": "project-1",
                    "threadId": "thread-1",
                    "goal": "Finish the parser.",
                    "mode": "autonomous",
                    "budget": {"maxContinuations": 2, "maxElapsedSeconds": 600},
                    "idempotencyKey": "run-1"
                }),
            ),
        ),
        (
            "statefulRun/pause",
            request("statefulRun/pause", run_control.clone()),
        ),
        (
            "statefulRun/resume",
            request("statefulRun/resume", run_control.clone()),
        ),
        (
            "statefulRun/cancel",
            request("statefulRun/cancel", run_control),
        ),
        (
            "statefulRun/setMode",
            request(
                "statefulRun/setMode",
                json!({"runId": "run-1", "expectedRevision": 1, "mode": "socratic"}),
            ),
        ),
        (
            "statefulMemory/forget",
            request(
                "statefulMemory/forget",
                json!({"threadId": "thread-1", "entryId": "entry-1", "expectedRevision": 1}),
            ),
        ),
        (
            "statefulMemory/correct",
            request(
                "statefulMemory/correct",
                json!({
                    "threadId": "thread-1",
                    "entryId": "entry-1",
                    "expectedRevision": 1,
                    "content": "Run only the affected tests."
                }),
            ),
        ),
    ]
}

#[test]
fn only_host_owned_connections_act_with_user_authority() {
    let origins = [
        ConnectionOrigin::InProcess,
        ConnectionOrigin::Stdio,
        ConnectionOrigin::WebSocket,
        ConnectionOrigin::RemoteControl,
    ];
    let mut decisions = Vec::new();
    for (method, request) in user_authority_requests() {
        for origin in origins {
            decisions.push((
                method,
                origin,
                require_user_authority(&request, origin).map_err(|error| error.message),
            ));
        }
    }
    let mut expected = Vec::new();
    for (method, _) in user_authority_requests() {
        let refused = Err(format!(
            "{method} acts with the user's authority and is available only to the host-owned stdio or in-process client; nothing was changed"
        ));
        expected.extend([
            (method, ConnectionOrigin::InProcess, Ok(())),
            (method, ConnectionOrigin::Stdio, Ok(())),
            (method, ConnectionOrigin::WebSocket, refused.clone()),
            (method, ConnectionOrigin::RemoteControl, refused),
        ]);
    }
    assert_eq!(decisions, expected);
}

#[test]
fn ordinary_requests_are_not_gated_by_origin() {
    let query = request(
        "blackboard/query",
        json!({"projectId": "project-1", "text": null, "withinNodeId": null, "limit": 5}),
    );
    for origin in [ConnectionOrigin::WebSocket, ConnectionOrigin::RemoteControl] {
        assert_eq!(
            require_user_authority(&query, origin).map_err(|error| error.message),
            Ok(())
        );
    }
}

fn method_params(request: &ClientRequest) -> (String, Value) {
    let value = serde_json::to_value(request).expect("request serializes");
    (
        value["method"].as_str().expect("method").to_string(),
        value["params"].clone(),
    )
}

/// Drives each privileged method through the real dispatcher from every origin,
/// with a client claiming to be the embedded TUI.
#[tokio::test]
async fn dispatch_refuses_network_peers_before_any_processor_work() -> anyhow::Result<()> {
    let mut gate_refusals = Vec::new();
    for origin in [
        ConnectionOrigin::InProcess,
        ConnectionOrigin::Stdio,
        ConnectionOrigin::WebSocket,
        ConnectionOrigin::RemoteControl,
    ] {
        let mut harness = Harness::new(origin, || false).await?;
        harness.initialize("codex-tui", /*opt_in*/ true).await;
        for (index, (method, request)) in user_authority_requests().into_iter().enumerate() {
            let (wire_method, params) = method_params(&request);
            harness
                .send(i64::try_from(index)? + 1, &wire_method, params)
                .await;
            let refused_by_gate = match harness.response().await {
                OutgoingMessage::Error(error) => {
                    error.error.code == -32600
                        && error
                            .error
                            .message
                            .starts_with(&format!("{method} acts with the user's authority"))
                }
                _ => false,
            };
            gate_refusals.push((origin, method, refused_by_gate));
        }
        harness.shutdown().await;
    }
    let mut expected = Vec::new();
    for origin in [
        ConnectionOrigin::InProcess,
        ConnectionOrigin::Stdio,
        ConnectionOrigin::WebSocket,
        ConnectionOrigin::RemoteControl,
    ] {
        let network_peer = matches!(
            origin,
            ConnectionOrigin::WebSocket | ConnectionOrigin::RemoteControl
        );
        for (method, _) in user_authority_requests() {
            expected.push((origin, method, network_peer));
        }
    }
    assert_eq!(gate_refusals, expected);
    Ok(())
}
