//! A remembered code conclusion that a later commit may have made untrue (horizon3's `y` ->
//! `yr`) is no longer presented as current: the next packet qualifies it with the commit as
//! the reason, reports the change, and leaves an unrelated `y` alone; recall says to check it.
//! A restarted server keeps the qualification.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

const SYMBOLS: &str =
    "Final compact duration unit symbols are y, mth, d, h; both formatters share them.";
const AXIS: &str = "The usage chart's y axis shows the request count.";

fn git(cwd: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=User",
            "-c",
            "user.email=user@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
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

fn units(years: &str) -> String {
    format!(
        "_COMPACT_UNITS = {{\n    \"YEARS\": \"{years}\",\n    \"MONTHS\": \"mth\",\n    \"DAYS\": \"d\",\n    \"HOURS\": \"h\",\n}}\n"
    )
}

#[tokio::test]
async fn a_commit_that_changes_a_remembered_value_retires_the_old_conclusion() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let repo = TempDir::new()?;
    git(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("_compact.py"), units("y"))?;
    git(repo.path(), &["add", "_compact.py"]);
    git(repo.path(), &["commit", "-q", "-m", "Add compact units"]);
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let fact = |key: &str, content: &str| {
        json!({
            "idempotencyKey": key,
            "kind": "fact",
            "content": content,
            "confidenceBasisPoints": 9000,
            "verification": "unverified",
            "importance": "high",
            "rootPromotion": "promoted"
        })
    };
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "record",
                "blackboard_record_batch",
                json!({"records": [fact("symbols", SYMBOLS), fact("axis", AXIS)]}),
            ),
            assistant("Recorded the symbols."),
            tool_call(
                "recall",
                "memory_read",
                json!({"question": "Which compact duration unit symbols do we use?"}),
            ),
            assistant("Years use yr now."),
            assistant("Still yr."),
        ],
    )
    .await;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Units".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(repo.path().to_path_buf())
                        .expect("absolute repository root"),
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "units-project".to_string(),
            },
        })
        .await?;
    let project_id = project.project.id;
    new_thread_turn(&mut server, &project_id, "Record the compact unit symbols.").await?;

    // Between sessions the user hand-edits the years symbol and commits with a reason.
    std::fs::write(repo.path().join("_compact.py"), units("yr"))?;
    git(
        repo.path(),
        &[
            "commit",
            "-q",
            "-am",
            "Hand edit: compact years symbol 'y' -> 'yr'",
        ],
    );
    new_thread_turn(&mut server, &project_id, "Which unit symbols do we use?").await?;
    drop(server);
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    new_thread_turn(&mut server, &project_id, "Remind me of the symbols.").await?;

    let requests = log.requests();
    let bodies = requests
        .iter()
        .map(|request| request.body_json().to_string())
        .collect::<Vec<_>>();
    let [_, _, second, _, third] = bodies.as_slice() else {
        panic!("five requests expected, got {}", bodies.len());
    };
    let shows = |body: &str, text: &str| body.contains(text);
    assert_eq!(
        (
            shows(
                second,
                "[may be outdated; check before relying on it: commit"
            ),
            shows(second, "changed _compact.py YEARS from"),
            shows(second, "1 remembered conclusion(s) may be outdated"),
            shows(second, "y axis shows the request count"),
            shows(
                third,
                "[may be outdated; check before relying on it: commit"
            ),
            shows(third, "unit symbols are y, mth"),
        ),
        (true, true, true, true, true, true)
    );

    let recall: Value = serde_json::from_str(
        &requests
            .iter()
            .find_map(|request| request.function_call_output_text("recall"))
            .expect("recall output"),
    )?;
    let statuses = recall["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter(|entry| {
            entry["content"]
                .as_str()
                .is_some_and(|content| content.starts_with("Final compact duration unit"))
        })
        .map(|entry| {
            entry["status"]
                .as_str()
                .unwrap_or_default()
                .starts_with("may be outdated; check before relying on it: commit")
        })
        .collect::<Vec<_>>();
    assert_eq!(statuses, vec![true]);
    Ok(())
}

async fn new_thread_turn(server: &mut TestAppServer, project_id: &str, text: &str) -> Result<()> {
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.to_string()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread,
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

fn tool_call(call_id: &str, tool: &str, arguments: Value) -> String {
    responses::sse(vec![
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(&format!("{call_id}-response")),
    ])
}

fn assistant(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("assistant-message", text),
        responses::ev_completed("assistant-response"),
    ])
}
