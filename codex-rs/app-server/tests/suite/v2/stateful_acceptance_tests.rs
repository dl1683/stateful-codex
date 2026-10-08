use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

const GOAL: &str = "All tests must pass.";

fn call(call_id: &str, tool: &str, arguments: Value) -> String {
    responses::sse(vec![
        responses::ev_response_created(call_id),
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(call_id),
    ])
}

fn exec(call_id: &str, command: &str) -> String {
    call(
        call_id,
        "exec_command",
        json!({"cmd": command, "yield_time_ms": 10_000}),
    )
}

fn complete(call_id: &str, revision: u64) -> String {
    call(
        call_id,
        "stateful_run_update",
        json!({
            "expectedRevision": revision,
            "status": "completed",
            "completionDisposition": "noReusableLearning",
            "result": "The tests pass.",
        }),
    )
}

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

async fn read_run(server: &mut TestAppServer, run_id: &str) -> Result<StatefulRun> {
    let read: StatefulRunReadResponse = server
        .request(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id.to_string()),
                thread_id: None,
            },
        })
        .await?;
    Ok(read.run.expect("run remains readable"))
}

async fn turn(server: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

/// Through the public API: a host-observed failing check keeps the run Running with the exact
/// unmet gates; after repair, current host evidence completes the run and the durable result
/// discloses the acceptance basis.
#[tokio::test]
async fn completion_requires_current_host_observed_acceptance_evidence() -> Result<()> {
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
                name: "Acceptance gate".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "acceptance-project".to_string(),
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
                goal: GOAL.to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "acceptance-run".to_string(),
            },
        })
        .await?;
    let revision = started.run.revision;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            call(
                "declare",
                "stateful_acceptance_update",
                json!({"expectedLedgerRevision": 0, "changes": [
                    {"action": "add", "origin": "user", "kind": "check", "statement": "The suite passes.", "requestQuote": GOAL, "checkCommand": "echo suite-ok", "expectedObservation": "the suite reports success"},
                    {"action": "add", "origin": "derived", "kind": "check", "statement": "The smoke check passes.", "checkCommand": "exit 3", "expectedObservation": "the smoke check exits 0"}
                ]}),
            ),
            exec("smoke-fails", "exit 3"),
            complete("premature", revision),
            message("refused", "The smoke check failed; I will repair it."),
            call(
                "repair",
                "stateful_acceptance_update",
                json!({"expectedLedgerRevision": 2, "changes": [
                    {"action": "refine", "criterion": "C2", "checkCommand": "echo smoke-ok"}
                ]}),
            ),
            exec("suite", "echo suite-ok"),
            exec("smoke", "echo smoke-ok"),
            complete("verified", revision),
            message("done", "The tests pass."),
        ],
    )
    .await;

    turn(&mut server, &thread.thread.id, "Make the tests pass.").await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 4);
    let refusal = requests[3].function_call_output("premature").to_string();
    for expected in [
        "completion refused; the run stays running",
        "C1 (The suite passes.): no current host evidence: run exactly `echo suite-ok`",
        "C2 (The smoke check passes.): `exit 3` failed (exit 3)",
    ] {
        assert!(
            refusal.contains(expected),
            "{expected} missing from {refusal}"
        );
    }
    assert_eq!(
        read_run(&mut server, &started.run.id).await?.status,
        StatefulRunStatus::Running
    );

    turn(&mut server, &thread.thread.id, "Repair and finish.").await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 9);
    let accepted = requests[8].function_call_output("verified").to_string();
    assert!(accepted.contains("acceptanceBasis"), "{accepted}");
    let run = read_run(&mut server, &started.run.id).await?;
    assert_eq!(run.status, StatefulRunStatus::Completed);
    let result = run.result.expect("durable result");
    assert!(
        result.starts_with("The tests pass.\n\nAcceptance basis:\n- C1 [user] The suite passes.: agent-written check `echo suite-ok` exited 0 on the host against the final workspace (expected: the suite reports success)"),
        "{result}"
    );
    assert!(
        result.contains(
            "C2 [derived] The smoke check passes.: agent-written check `echo smoke-ok` exited 0 on the host"
        ),
        "{result}"
    );
    Ok(())
}
