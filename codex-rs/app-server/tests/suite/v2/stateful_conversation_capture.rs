//! Conversation capture for recall (build item 4): a completed answer's ruled-out list, open
//! checks and decisions are saved by the host with no model memory call, and after more
//! unrelated turns than the continuity window holds, one memory_read in a new thread returns
//! every ruled-out item whole, in order, with complete coverage; the open check is still open
//! and the decision keeps its recorded reason.

use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

const OPENING: &str = "Two new laptops fail `shipit deploy` with a config file not found error. Please investigate and find the root cause. Keep a running list of the hypotheses you have ruled out.";

/// debug2's session-1 answer, with an open check and a decision added.
const ANSWER: &str = "Root cause found: the vendored Click records the config parameter source after conversion.

What was ruled out:

- Environment variable `SHIPIT_CONFIG`: absent during reproduction.
- Incorrect home-path expansion: the expected temporary home path was used.
- TOML parsing, built-in defaults, and deploy planning: the failure occurs before those stages; `load_config(None)` returns the built-in defaults.
- `--env` and `--dry-run`: both variants fail identically.
- Test/runtime code changes: the worktree remains clean.

Open checks:
- Confirm the tests import the vendored click, not the installed one.

Decision: fix the ordering in the vendored Click rather than work around it in `ConfigPath`.
Reason: both symptoms come from the same ordering.

No code has been changed.";

const RULED_OUT: [&str; 5] = [
    "Environment variable `SHIPIT_CONFIG`: absent during reproduction.",
    "Incorrect home-path expansion: the expected temporary home path was used.",
    "TOML parsing, built-in defaults, and deploy planning: the failure occurs before those stages; `load_config(None)` returns the built-in defaults.",
    "`--env` and `--dry-run`: both variants fail identically.",
    "Test/runtime code changes: the worktree remains clean.",
];

/// More unrelated turns than the newest-ten continuity window.
const UNRELATED_TURNS: usize = 11;

#[tokio::test]
async fn a_completed_answer_is_recalled_whole_after_unrelated_work() -> Result<()> {
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
                name: "Capture".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "capture-project".to_string(),
            },
        })
        .await?;
    let mut script = vec![assistant(ANSWER)];
    script.extend((0..UNRELATED_TURNS).map(|_| assistant("Done.")));
    script.extend([
        tool_call(
            "recall-ruled-out",
            "memory_read",
            json!({"question": "What have we ruled out so far about the config error?"}),
        ),
        tool_call("recall-checks", "memory_read", json!({"kind": "openCheck"})),
        tool_call(
            "recall-decision",
            "memory_read",
            json!({"kind": "decision"}),
        ),
        assistant("Here is what we ruled out."),
    ]);
    let log = responses::mount_sse_sequence(&responses_server, script).await;

    let investigation = start_thread(&mut server, &project.project.id).await?;
    run_turn(&mut server, &investigation, OPENING).await?;
    let unrelated = start_thread(&mut server, &project.project.id).await?;
    for index in 0..UNRELATED_TURNS {
        run_turn(
            &mut server,
            &unrelated,
            &format!("Rename helper number {index} in utils.py to snake_case."),
        )
        .await?;
    }
    let returning = start_thread(&mut server, &project.project.id).await?;
    run_turn(
        &mut server,
        &returning,
        "It's been a few days. What have we ruled out so far about the config error?",
    )
    .await?;

    // The scripted model never calls a memory write tool: every save below is the host's.
    let requests = log.requests();
    let output = |call_id: &str| -> Result<Value> {
        Ok(serde_json::from_str(
            &requests
                .iter()
                .find_map(|request| request.function_call_output_text(call_id))
                .expect("tool output"),
        )?)
    };
    let field = |result: &Value, name: &str| -> Vec<Value> {
        result["requested"]["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| item[name].clone())
            .collect()
    };
    let ruled_out = output("recall-ruled-out")?;
    let checks = output("recall-checks")?;
    let decisions = output("recall-decision")?;
    assert_eq!(
        (
            field(&ruled_out, "content"),
            ruled_out["requested"]["coverage"]["complete"].clone(),
            field(&checks, "checkState"),
            field(&decisions, "reasonStatus"),
        ),
        (
            RULED_OUT.iter().map(|item| json!(item)).collect::<Vec<_>>(),
            json!(true),
            vec![json!("open")],
            vec![json!("recorded")],
        )
    );
    assert!(
        field(&decisions, "content")[0]
            .as_str()
            .unwrap_or_default()
            .contains("Reason: both symptoms come from the same ordering."),
    );
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

async fn start_thread(server: &mut TestAppServer, project_id: &str) -> Result<String> {
    Ok(server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.to_string()),
            ..Default::default()
        })
        .await?
        .thread
        .id)
}

async fn run_turn(server: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
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
