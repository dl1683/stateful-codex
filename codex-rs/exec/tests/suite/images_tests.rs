use core_test_support::responses;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ResponsesRequest;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex_exec::test_codex_exec;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::MockServer;

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

async fn mount_response(server: &MockServer) -> ResponseMock {
    responses::mount_sse_once(
        server,
        responses::sse(vec![
            responses::ev_response_created("resp-image"),
            responses::ev_assistant_message("msg-image", "image response"),
            responses::ev_completed("resp-image"),
        ]),
    )
    .await
}

fn last_user_content(request: &ResponsesRequest) -> Value {
    request
        .input()
        .into_iter()
        .rev()
        .find(|item| item["role"] == "user")
        .expect("request should contain a user message")["content"]
        .clone()
}

fn started_thread_id(stdout: &[u8]) -> anyhow::Result<String> {
    let event: Value = serde_json::from_slice(
        stdout
            .split(|byte| *byte == b'\n')
            .next()
            .expect("exec should emit a thread.started event"),
    )?;
    assert_eq!(event["type"], "thread.started");
    Ok(event["thread_id"]
        .as_str()
        .expect("thread.started should include the thread id")
        .to_string())
}

#[test]
fn exec_help_lists_image_input() {
    test_codex_exec()
        .cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("  -i, --image <FILE>\n"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_run_image_input_matches_fork_and_resume() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let image_path = test.cwd_path().join("page.png");
    std::fs::write(&image_path, TINY_PNG)?;
    let prompt = "describe the page";
    let response_mock = mount_response(&server).await;
    let output = test
        .cmd_with_server(&server)
        .args(["--skip-git-repo-check", "--json", "-i"])
        .arg(&image_path)
        .arg(prompt)
        .write_stdin("")
        .assert()
        .success()
        .get_output()
        .clone();
    let source_id = started_thread_id(&output.stdout)?;
    let expected = last_user_content(&response_mock.single_request());
    let images = expected
        .as_array()
        .expect("message content should be an array")
        .iter()
        .filter(|item| item["type"] == "input_image")
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 1);
    assert!(
        images[0]["image_url"]
            .as_str()
            .expect("image should have an inline URL")
            .starts_with("data:image/png;base64,")
    );
    assert_eq!(
        expected.as_array().and_then(|content| content.last()),
        Some(&json!({"type": "input_text", "text": prompt}))
    );

    for command in ["fork", "resume"] {
        let prompt = format!("{command}: describe the page again");
        let mut expected = expected.clone();
        *expected
            .as_array_mut()
            .and_then(|content| content.last_mut())
            .expect("message should end with the prompt") =
            json!({"type": "input_text", "text": prompt});
        let response_mock = mount_response(&server).await;
        test.cmd_with_server(&server)
            .args(["--skip-git-repo-check", command])
            .arg(&source_id)
            .arg("--image")
            .arg(&image_path)
            .arg(&prompt)
            .assert()
            .success();
        assert_eq!(
            last_user_content(&response_mock.single_request()),
            expected,
            "{command} should attach the image and its new prompt"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stateful_modes_attach_image_and_prompt_to_first_request() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    for mode in ["ask", "autonomous", "collaborative", "socratic"] {
        let test = test_codex_exec();
        let server = responses::start_mock_server().await;
        let image_path = test.cwd_path().join("page.png");
        std::fs::write(&image_path, TINY_PNG)?;
        let prompt = format!("{mode}: describe the page");
        // Let Autonomous end answered rather than scheduling another model turn.
        let response_mock = responses::mount_sse_once(
            &server,
            responses::sse(vec![
                responses::ev_response_created("resp-image"),
                responses::ev_assistant_message(
                    "msg-image",
                    "The image is a tiny PNG.\n\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]",
                ),
                responses::ev_completed("resp-image"),
            ]),
        )
        .await;
        test.cmd_with_server(&server)
            .args(["--skip-git-repo-check", "--stateful", mode, "--image"])
            .arg(&image_path)
            .arg(&prompt)
            .assert()
            .success();

        let content = last_user_content(&response_mock.single_request());
        let content = content
            .as_array()
            .expect("message content should be an array");
        let images = content
            .iter()
            .filter(|item| item["type"] == "input_image")
            .collect::<Vec<_>>();
        assert_eq!(images.len(), 1, "{mode} should attach the image");
        assert!(
            images[0]["image_url"]
                .as_str()
                .expect("image should have an inline URL")
                .starts_with("data:image/png;base64,"),
            "{mode} should send the PNG inline"
        );
        assert_eq!(
            content.last(),
            Some(&json!({"type": "input_text", "text": prompt})),
            "{mode} should send the prompt after the image"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_run_missing_image_matches_fork_and_resume() -> anyhow::Result<()> {
    assert_image_error_matches_fork_and_resume(ImageFailure::MissingFile).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_run_non_image_matches_fork_and_resume() -> anyhow::Result<()> {
    assert_image_error_matches_fork_and_resume(ImageFailure::NonImage).await
}

enum ImageFailure {
    MissingFile,
    NonImage,
}

async fn assert_image_error_matches_fork_and_resume(failure: ImageFailure) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let test = test_codex_exec();
    let server = responses::start_mock_server().await;
    let (path, expected_error) = match failure {
        ImageFailure::MissingFile => {
            let path = test.cwd_path().join("missing.png");
            let read_error = std::fs::read(&path).expect_err("image should not exist");
            let message = format!(
                "Codex could not read the local image at `{}`: {read_error}",
                path.display()
            );
            (path, message)
        }
        ImageFailure::NonImage => {
            let path = test.cwd_path().join("not_an_image.txt");
            std::fs::write(&path, "not an image")?;
            (
                path,
                "image content omitted because it could not be processed".to_string(),
            )
        }
    };
    let prompt = "describe the page";
    let response_mock = mount_response(&server).await;
    let output = test
        .cmd_with_server(&server)
        .args(["--skip-git-repo-check", "--json", "--image"])
        .arg(&path)
        .arg(prompt)
        .write_stdin("")
        .assert()
        .success()
        .get_output()
        .clone();
    let source_id = started_thread_id(&output.stdout)?;
    let expected = last_user_content(&response_mock.single_request());
    let content = expected.as_array().expect("message content array");
    assert!(
        content
            .iter()
            .any(|item| { item["type"] == "input_text" && item["text"] == expected_error })
    );
    assert!(content.iter().all(|item| item["type"] != "input_image"));
    assert_eq!(
        content.last(),
        Some(&json!({"type": "input_text", "text": prompt}))
    );

    // Existing exec paths reject the attachment with a request placeholder, not an exit error.
    for command in ["fork", "resume"] {
        let prompt = format!("{command}: describe the page again");
        let mut expected = expected.clone();
        *expected
            .as_array_mut()
            .and_then(|content| content.last_mut())
            .expect("message should end with the prompt") =
            json!({"type": "input_text", "text": prompt});
        let response_mock = mount_response(&server).await;
        test.cmd_with_server(&server)
            .args(["--skip-git-repo-check", command])
            .arg(&source_id)
            .arg("--image")
            .arg(&path)
            .arg(&prompt)
            .assert()
            .success();
        assert_eq!(
            last_user_content(&response_mock.single_request()),
            expected,
            "{command} should report the same image error as a new run"
        );
    }
    Ok(())
}
