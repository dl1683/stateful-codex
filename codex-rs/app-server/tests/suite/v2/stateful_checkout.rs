//! Changes made to the checkout between Stateful turns are reported to the next turn once,
//! with their origin left unknown and commit messages carried as the stated reasons.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=User",
            "-c",
            "user.email=user@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn checkout_changes_between_turns_reach_the_next_turn_once() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let repo = TempDir::new()?;
    git(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("rate.py"), "ONE = 1\n")?;
    git(repo.path(), &["add", "rate.py"]);
    git(repo.path(), &["commit", "-q", "-m", "initial"]);
    MockResponsesConfig::new(&responses.uri())
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
                name: "Checkout".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(repo.path().to_path_buf())
                        .expect("absolute repository root"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "checkout-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(&mut server, &thread).await?;

    // Between turns: a commit with its reason, and an uncommitted note.
    std::fs::write(repo.path().join("rate.py"), "TYPO_HINTS = 1\n")?;
    git(repo.path(), &["add", "rate.py"]);
    git(
        repo.path(),
        &[
            "commit",
            "-q",
            "-m",
            "Rename suggest_typos to typo_hints",
            "-m",
            "Matches the key our downstream app uses.",
        ],
    );
    std::fs::write(repo.path().join("NOTES-rate.md"), "Priya asked for per=w\n")?;
    run_turn(&mut server, &thread).await?;
    run_turn(&mut server, &thread).await?;

    let bodies = responses
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|request| request.url.path().ends_with("/responses"))
        .map(|request| {
            request
                .body_json::<serde_json::Value>()
                .map(|body| body.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [first, second, third] = bodies.as_slice() else {
        panic!("three requests expected, got {}", bodies.len());
    };
    let count = |body: &str| body.matches("<stateful_checkout_changes>").count();
    assert_eq!(
        (
            count(first),
            count(second),
            count(third),
            second.contains("Rename suggest_typos to typo_hints"),
            second.contains("Matches the key our downstream app uses."),
            second.contains("NOTES-rate.md"),
            second.contains("Their origin is unknown"),
        ),
        (0, 1, 1, true, true, true, true)
    );
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
