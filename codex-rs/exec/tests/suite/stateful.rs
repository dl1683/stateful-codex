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

/// Ask (a bare `--stateful`, here after the prompt) attaches the project's memory and starts
/// no run: the request carries the project section and the memory tool but no run section and
/// no run-bound tool.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_stateful_ask_attaches_project_memory_without_a_run() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("ask"),
            responses::ev_assistant_message("ask-message", "Paris."),
            responses::ev_completed("ask"),
        ]),
    )
    .await;

    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("What is the capital of France?")
        .arg("--stateful")
        .assert()
        .success();

    let request = response_mock.single_request();
    assert_eq!(
        (
            request.body_contains_text("<stateful_project>"),
            request.body_contains_text("memory_read"),
            request.body_contains_text("<stateful_run>"),
            request.body_contains_text("stateful_run_update"),
            request.body_contains_text("stateful_acceptance_update"),
        ),
        (true, true, false, false, false)
    );
    Ok(())
}

/// An Autonomous run that ends with a ready answer prints the answer without its outcome
/// block and the calm outcome line, and exits successfully.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_autonomous_ready_answer_is_calm_and_hides_the_outcome_block() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("answer"),
            responses::ev_assistant_message(
                "answer-message",
                "A leap year has 366 days.

[stateful-outcome]
disposition: answer
open-issues: none
[/stateful-outcome]",
            ),
            responses::ev_completed("answer"),
        ]),
    )
    .await;

    let assertion = test
        .cmd_with_server(&server)
        .arg("--stateful")
        .arg("autonomous")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("How many days are in a leap year?")
        .assert()
        .success();

    response_mock.single_request();
    let output = assertion.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        (
            String::from_utf8_lossy(&output.stdout).to_string(),
            stderr.contains("run answered · not verified"),
            stderr.contains("[stateful-outcome]"),
        ),
        (
            "A leap year has 366 days.
"
            .to_string(),
            true,
            false
        )
    );
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

// The admitted acceptance check runs `sh`.
#[cfg(not(target_os = "windows"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_autonomous_stateful_follows_continuations_until_completion() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    // Completion needs a covering criterion settled by its admitted check; the project root
    // (the working directory) holds only the files that check pins.
    let goal = "Finish a task that requires another model turn";
    let root = test.cwd_path().to_path_buf();
    std::fs::write(root.join("accepted.txt"), "accepted\n")?;
    std::fs::write(root.join("verify.sh"), "test -s accepted.txt\n")?;
    // The host identifies a check's workspace only inside a git work tree.
    let initialized = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&root)
        .status()?;
    anyhow::ensure!(initialized.success(), "git init failed");
    let call = |id: &str, tool: &str, arguments: serde_json::Value| {
        responses::sse(vec![
            responses::ev_response_created(id),
            responses::ev_function_call(id, tool, &arguments.to_string()),
            responses::ev_completed(id),
        ])
    };
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
            call(
                "declare",
                "stateful_acceptance_update",
                serde_json::json!({"expectedLedgerRevision": 0, "changes": [{
                    "action": "add", "origin": "user", "kind": "deliverable",
                    "statement": "The task is finished.", "requestQuote": goal,
                    "checkCommand": "sh verify.sh", "expectedObservation": "exit 0",
                    "artifacts": ["accepted.txt"], "checker": ["verify.sh"]
                }]}),
            ),
            call(
                "admit",
                "stateful_acceptance_update",
                serde_json::json!({"expectedLedgerRevision": 1, "changes": [
                    {"action": "admit", "criterion": "C1"}
                ]}),
            ),
            call(
                "check",
                "exec_command",
                serde_json::json!({"cmd": "sh verify.sh", "workdir": root.to_string_lossy(), "yield_time_ms": 10_000}),
            ),
            responses::sse(vec![
                responses::ev_response_created("response-2"),
                responses::ev_function_call(
                    "complete-autonomous-run",
                    "stateful_run_update",
                    r#"{"expectedRevision": 2, "status": "completed", "openIssues": [], "result": "The autonomous investigation is complete.", "rootRevision": 0, "materialRootFindings": [], "completionIdempotencyKey": "autonomous-final", "finalObligation": {"learning": [], "implication": ["No further continuation is required."]}}"#,
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
        .arg(goal)
        .assert()
        .success();

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 6);
    assert!(!requests[0].body_contains_text("Continue Autonomous Stateful run"));
    assert!(requests[1].body_contains_text("Continue Autonomous Stateful run"));
    assert!(requests[5].body_contains_text("finalAnswerInstruction"));
    assert!(requests[5].body_contains_text("The autonomous investigation is complete."));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exec_resume_joins_an_open_collaborative_run_only_without_the_flag() -> anyhow::Result<()> {
    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let first_response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-1"),
            responses::ev_assistant_message("message-1", "first turn answered"),
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
        .arg("Establish the collaborative goal")
        .assert()
        .success();
    first_response.single_request();

    // The answered turn leaves the Collaborative run open, so a second run is refused with
    // the control that continues the open one.
    let refused = test
        .cmd_with_server(&server)
        .arg("--stateful")
        .arg("collaborative")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("resume")
        .arg("--last")
        .arg("Start another collaborative goal")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(
        stderr.contains("already has an open Stateful run")
            && stderr.contains("resume the thread without --stateful"),
        "stderr should name the open run and how to continue it: {stderr}"
    );

    // A plain resume joins the open run: its turn carries the run packet and policy.
    let second_response = responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("response-2"),
            responses::ev_assistant_message("message-2", "second turn answered"),
            responses::ev_completed("response-2"),
        ]),
    )
    .await;
    test.cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("resume")
        .arg("--last")
        .arg("Continue the collaborative goal")
        .assert()
        .success();
    let request = second_response.single_request();
    assert!(request.body_contains_text("Continue the collaborative goal"));
    assert!(request.body_contains_text("Establish the collaborative goal"));
    assert!(request.body_contains_text("stays open across the user's turns"));

    // A different mode is refused with the open run's mode.
    let refused = test
        .cmd_with_server(&server)
        .arg("--stateful")
        .arg("autonomous")
        .arg("--skip-git-repo-check")
        .arg("-C")
        .arg(test.cwd_path())
        .arg("resume")
        .arg("--last")
        .arg("Take over autonomously")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(
        stderr.contains("it runs in collaborative mode"),
        "stderr should name the open run's mode: {stderr}"
    );
    Ok(())
}
