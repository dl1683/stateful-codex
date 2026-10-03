//! One evidence-bearing recall (council slice 4): a question about a changed decision is
//! answered from one memory_read result carrying the current value, what it replaced and
//! when, and the earlier turn where the user changed it.

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

const ONE: &str = "Default rate formatting uses ONE decimal place.";
const TWO: &str = "Default rate formatting uses TWO decimal places because the dashboard compares sub-unit rates.";
const CHANGE_REQUEST: &str = "Requirement changed: default formatting must now use TWO decimal places, because the dashboard compares sub-unit rates.";

#[tokio::test]
async fn one_memory_read_returns_the_current_decision_its_history_and_the_users_reason()
-> Result<()> {
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
                name: "Recall".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "recall-project".to_string(),
            },
        })
        .await?;
    let decision = |key: &str, content: &str, supersedes: Value| {
        json!({"records": [{
            "idempotencyKey": key,
            "kind": "decision",
            "content": content,
            "confidenceBasisPoints": 9000,
            "verification": "unverified",
            "importance": "high",
            "rootPromotion": "promoted",
            "supersedes": supersedes
        }]})
    };
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "record-one",
                "blackboard_record_batch",
                decision("one", ONE, json!([])),
            ),
            assistant("Recorded ONE."),
            tool_call(
                "record-two",
                "blackboard_record_batch",
                decision("two", TWO, json!([{"alias": "E1"}])),
            ),
            assistant("Recorded TWO."),
            tool_call(
                "recall",
                "memory_read",
                json!({"question": "Why is the default rate formatting two decimal places?"}),
            ),
            assistant("TWO decimal places; it replaced ONE when the requirement changed."),
            tool_call(
                "recall-since",
                "memory_read",
                json!({"since": "2000-01-01"}),
            ),
            assistant("Summary written."),
        ],
    )
    .await;
    for prompt in [
        "Default formatting must use ONE decimal place.",
        CHANGE_REQUEST,
        "What is our default decimal places setting and how did we arrive at it?",
        "Summarize what we decided since the start of the project.",
    ] {
        let thread = server
            .start_thread(ThreadStartParams {
                project_id: Some(project.project.id.clone()),
                ..Default::default()
            })
            .await?
            .thread
            .id;
        run_turn(&mut server, &thread, prompt).await?;
    }
    let requests = log.requests();
    let output = |call_id: &str| -> Result<Value> {
        Ok(serde_json::from_str(
            &requests
                .iter()
                .find_map(|request| request.function_call_output_text(call_id))
                .expect("tool output"),
        )?)
    };
    let recall = output("recall")?;
    let entries = recall["entries"].as_array().expect("entries");
    let statuses = entries
        .iter()
        .map(|entry| {
            (
                entry["content"].as_str().unwrap_or_default().to_string(),
                entry["status"]
                    .as_str()
                    .unwrap_or_default()
                    .split(" on ")
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    let two_id = entries[0]["entryId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        statuses,
        vec![
            (TWO.to_string(), "current".to_string()),
            (ONE.to_string(), format!("replaced by {two_id}")),
        ]
    );
    let turns = recall["turns"].as_array().expect("turns");
    assert!(
        turns
            .iter()
            .any(|turn| turn["user"].as_str() == Some(CHANGE_REQUEST)),
        "{turns:?}"
    );
    assert_eq!(entries[0]["source"], json!("agent record"));
    // History rows carry the same authority fields as current ones.
    assert_eq!(
        (entries[1]["kind"].clone(), entries[1]["source"].clone()),
        (json!("decision"), json!("agent record"))
    );

    let since = output("recall-since")?;
    assert!(
        since["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["content"] == json!(TWO))
    );
    assert!(since["turns"].as_array().expect("turns").len() >= 3);
    assert!(since.to_string().len() <= 9_000);
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
