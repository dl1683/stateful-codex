#![allow(clippy::expect_used)]

use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use pretty_assertions::assert_eq;

fn completed_model_responses(stdout: &[u8]) -> Option<u64> {
    completed_event(stdout).and_then(|event| {
        event
            .pointer("/trajectory/completed_model_responses")
            .and_then(serde_json::Value::as_u64)
    })
}

fn completed_event(stdout: &[u8]) -> Option<serde_json::Value> {
    String::from_utf8_lossy(stdout).lines().find_map(|line| {
        let event: serde_json::Value = serde_json::from_str(line).ok()?;
        (event.get("type")?.as_str()? == "turn.completed").then_some(event)
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_stateful_with_positional_prompt_does_not_wait_for_open_stdin() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-1"),
            responses::ev_assistant_message("message-1", "done"),
            responses::ev_completed("response-1"),
        ]),
    )
    .await;
    let prompt = "Investigate without reading ambient stdin";

    let mut command = test.cmd_with_server(&server);
    command
        .arg("--stateful")
        .arg("collaborative")
        .arg("--json")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg(prompt);
    let mut child_command = tokio::process::Command::new(command.get_program());
    child_command
        .args(command.get_args())
        .envs(
            command
                .get_envs()
                .filter_map(|(key, value)| value.map(|value| (key, value))),
        )
        .current_dir(test.cwd_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = child_command.spawn()?;
    let _open_stdin = child.stdin.take().expect("stdin should be piped");
    let output = tokio::time::timeout(Duration::from_secs(/*secs*/ 90), child.wait_with_output())
        .await
        .context("Stateful exec should not wait for ambient stdin to close")??;

    assert!(
        output.status.success(),
        "Stateful exec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = response_mock.single_request();
    assert!(request.has_message_with_input_texts("user", |texts| texts == [prompt.to_string()]));
    assert_eq!(completed_model_responses(&output.stdout), Some(1));
    assert_eq!(
        completed_event(&output.stdout)
            .as_ref()
            .and_then(|event| event.pointer("/stateful_attribution/completed_turns"))
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_exec_reports_neutral_trajectory_without_stateful_attribution()
-> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-1"),
            responses::ev_assistant_message("message-1", "done"),
            responses::ev_completed("response-1"),
        ]),
    )
    .await;

    let assertion = test
        .cmd_with_server(&server)
        .arg("--json")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("Measure an ordinary run")
        .assert()
        .success();

    response_mock.single_request();
    let completed = completed_event(&assertion.get_output().stdout).expect("turn.completed event");
    assert_eq!(
        completed
            .pointer("/trajectory/completed_model_responses")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert!(completed.get("stateful_attribution").is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_stateful_starts_the_run_before_the_first_model_request() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-1"),
            responses::ev_assistant_message("message-1", "done"),
            responses::ev_completed("response-1"),
        ]),
    )
    .await;

    test.cmd_with_server(&server)
        .arg("--stateful")
        .arg("collaborative")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("Investigate the selected project")
        .assert()
        .success();

    let request = response_mock.single_request();
    assert!(request.body_contains_text("evidence_read"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_stateful_resume_starts_a_new_run_for_the_new_prompt() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let first_response = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("response-1"),
                responses::ev_function_call(
                    "finish-first-run",
                    "stateful_run_update",
                    r#"{"expectedRevision": 1, "status": "failed", "result": "test terminal result"}"#,
                ),
                responses::ev_completed_with_tokens("response-1", /*total_tokens*/ 60),
            ]),
            responses::sse(vec![
                responses::ev_response_created("response-2"),
                responses::ev_assistant_message("message-2", "first run finished"),
                responses::ev_completed_with_tokens("response-2", /*total_tokens*/ 40),
            ]),
        ],
    )
    .await;

    test.cmd_with_server(&server)
        .arg("--stateful")
        .arg("collaborative")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("Establish the first Stateful goal")
        .assert()
        .success();

    let second_response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-3"),
            responses::ev_assistant_message("message-3", "second run active"),
            responses::ev_completed_with_tokens("response-3", /*total_tokens*/ 30),
        ]),
    )
    .await;
    let assertion = test
        .cmd_with_server(&server)
        .arg("--stateful")
        .arg("collaborative")
        .arg("--json")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("resume")
        .arg("--last")
        .arg("Investigate the second Stateful goal")
        .assert()
        .success();

    assert_eq!(first_response.requests().len(), 2);
    let request = second_response.single_request();
    assert!(request.body_contains_text("Investigate the second Stateful goal"));
    assert!(request.body_contains_text("<stateful_run>"));
    assert!(request.body_contains_text("stateful_run_update"));
    assert!(!request.body_contains_text("Continue Autonomous Stateful run"));
    assert_eq!(
        completed_model_responses(&assertion.get_output().stdout),
        Some(1)
    );
    let completed = completed_event(&assertion.get_output().stdout).expect("turn.completed event");
    assert_eq!(
        completed
            .pointer("/usage/input_tokens")
            .and_then(serde_json::Value::as_i64),
        Some(30)
    );
    assert_eq!(
        completed
            .pointer("/stateful_attribution/completed_turns")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_autonomous_stateful_follows_continuations_until_completion() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("response-1"),
                responses::ev_assistant_message(
                    "message-1",
                    "I found useful work and need another turn.",
                ),
                responses::ev_completed("response-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("response-2"),
                responses::ev_function_call(
                    "complete-autonomous-run",
                    "stateful_run_update",
                    r#"{"expectedRevision": 2, "status": "completed", "result": "The autonomous investigation is complete.", "rootRevision": 0, "materialRootFindings": [], "completionIdempotencyKey": "autonomous-final", "finalObligation": {"learning": [], "implication": ["No further continuation is required."]}}"#,
                ),
                responses::ev_completed("response-2"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("response-3"),
                responses::ev_assistant_message(
                    "message-3",
                    "The autonomous investigation is complete.",
                ),
                responses::ev_completed("response-3"),
            ]),
        ],
    )
    .await;

    test.cmd_with_server(&server)
        .arg("--stateful")
        .arg("autonomous")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("Finish a task that requires another model turn")
        .assert()
        .success();

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].body_contains_text("Continue Autonomous Stateful run"));
    assert!(requests[1].body_contains_text("Continue Autonomous Stateful run"));
    assert!(requests[2].body_contains_text("finalAnswerInstruction"));
    assert!(requests[2].body_contains_text("The autonomous investigation is complete."));
    Ok(())
}
