#![allow(clippy::expect_used)]

use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use pretty_assertions::assert_eq;

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
                responses::ev_custom_tool_call(
                    "finish-first-run",
                    "exec",
                    r#"const result = await tools.stateful_run_update({expectedRevision: 1, status: "failed", result: "test terminal result"}); text(JSON.stringify(result));"#,
                ),
                responses::ev_completed("response-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("response-2"),
                responses::ev_assistant_message("message-2", "first run finished"),
                responses::ev_completed("response-2"),
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
            responses::ev_completed("response-3"),
        ]),
    )
    .await;
    test.cmd_with_server(&server)
        .arg("--stateful")
        .arg("collaborative")
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
                responses::ev_custom_tool_call(
                    "complete-autonomous-run",
                    "exec",
                    r#"const result = await tools.stateful_run_update({expectedRevision: 2, status: "completed", result: "The autonomous investigation is complete.", rootRevision: 0, materialRootFindings: [], completionIdempotencyKey: "autonomous-final", finalObligation: {learning: ["The autonomous investigation reached its result."], implication: ["No further continuation is required."]}}); text(JSON.stringify(result));"#,
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
