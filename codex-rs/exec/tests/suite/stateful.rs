#![allow(clippy::expect_used)]

use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;

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
