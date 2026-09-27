use core_test_support::responses;
use core_test_support::test_codex_exec::test_codex_exec;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_config_warning_is_emitted_once_in_json_output() -> anyhow::Result<()> {
    let test = test_codex_exec();
    std::fs::write(
        test.home_path().join("config.toml"),
        "unknown_startup_setting = true\n",
    )?;
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp1"),
            responses::ev_assistant_message("m1", "fixture hello"),
            responses::ev_completed("resp1"),
        ]),
    )
    .await;

    let output = test
        .cmd_with_server(&server)
        .arg("--skip-git-repo-check")
        .arg("--experimental-json")
        .arg("say hello")
        .output()?;
    assert!(
        output.status.success(),
        "codex exec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let warning_count = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event
                .pointer("/item/type")
                .and_then(serde_json::Value::as_str)
                == Some("error")
                && event
                    .pointer("/item/message")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|message| message.contains("unrecognized configuration setting"))
        })
        .count();

    assert_eq!(warning_count, 1);
    Ok(())
}
