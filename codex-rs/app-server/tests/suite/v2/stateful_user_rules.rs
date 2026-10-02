//! Exact capture of the user's rules (council slice 2): the host stores rules the user marks
//! as standing in their own words, the model can add other rules only by quoting the user,
//! one-off directions and invented preferences never become rules, and a fresh thread's
//! packet leads with the user's exact words.

use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

/// The tui7 opening message: three rules in prose, an unmarked environment rule, and a
/// one-off restriction for the orientation.
const OPENING: &str = "Morning! Before writing anything I'd like you to get oriented. A couple of ways I like to work, so you know: I review and commit everything myself, so please never run git commit or anything that rewrites history. Also, don't run the whole test suite every time - just run the test file(s) relevant to what you changed. The test env is the venv one level up (../venv). No code changes yet, just the exploration and the plan. Oh, and one more thing: end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.";

#[tokio::test]
async fn user_rules_are_kept_in_the_users_words_and_nothing_else_becomes_a_rule() -> Result<()> {
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
                name: "User rules".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "user-rules-project".to_string(),
            },
        })
        .await?;
    let first = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let record = |key: &str, quote: &str, scope: &str| {
        json!({
            "idempotencyKey": key,
            "kind": "instruction",
            "content": quote,
            "confidenceBasisPoints": 10000,
            "verification": "unverified",
            "importance": "high",
            "rootPromotion": "promoted",
            "userQuote": quote,
            "ruleScope": scope
        })
    };
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "record-rules",
                "blackboard_record_batch",
                json!({"records": [
                    // An unmarked rule, quoted exactly: stored as the whole sentence.
                    record("venv", "the venv one level up", "standing"),
                    // The one-off, wrongly claimed as standing: the user limited it ("yet").
                    record("orientation", "No code changes yet", "standing"),
                    // An invented preference has no source in the user's words.
                    record("invented", "Prefer concise progress updates", "standing"),
                ]}),
            ),
            assistant("Recorded.\nNext: map the package layout."),
            assistant("Renamed.\nNext: run the relevant tests."),
        ],
    )
    .await;
    run_turn(&mut server, &first, OPENING).await?;
    let output: Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("record-rules")
            .expect("record output"),
    )?;
    let outcomes = output["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|result| (result["recorded"].clone(), result["error"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        vec![
            (json!(true), Value::Null),
            (
                json!(false),
                json!("nothing written: the user limited that sentence to the current task")
            ),
            (
                json!(false),
                json!(
                    "userQuote is not inside exactly one complete sentence of a user message recorded in this thread"
                )
            ),
        ]
    );
    // Receipts name what was saved, in the user's words, for this turn.
    let mut receipts = Vec::new();
    for _ in 0..4 {
        let receipt: StatefulKnowledgeCapturedNotification = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            server.read_notification("statefulKnowledge/captured"),
        )
        .await??;
        assert_eq!(receipt.thread_id, first);
        receipts.push((receipt.category, receipt.outcome, receipt.text));
    }
    receipts.sort_by(|left, right| left.2.cmp(&right.2));
    assert_eq!(
        receipts,
        vec![
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "A couple of ways I like to work, so you know: I review and commit everything myself, so please never run git commit or anything that rewrites history.".to_string(),
            ),
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "Also, don't run the whole test suite every time - just run the test file(s) relevant to what you changed.".to_string(),
            ),
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "Oh, and one more thing: end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.".to_string(),
            ),
            (
                StatefulKnowledgeCategory::Rule,
                StatefulCaptureOutcome::Stored,
                "The test env is the venv one level up (../venv).".to_string(),
            ),
        ]
    );
    // The host captured the marked rules before the first request of the same turn.
    let first_request = log.requests()[0].body_json().to_string();
    assert!(first_request.contains("please never run git commit"));

    // A fresh thread with a narrow request: the packet leads with the user's exact words.
    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    run_turn(
        &mut server,
        &fresh,
        "Rename the helper in utils.py to snake_case and update its callers",
    )
    .await?;
    let fresh_request = log.requests()[2].body_json().to_string();
    let rules = [
        "please never run git commit or anything that rewrites history.",
        "don't run the whole test suite every time - just run the test file(s) relevant to what you changed.",
        "The test env is the venv one level up (../venv).",
        "end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.",
        "No code changes yet",
        "Prefer concise progress updates",
    ]
    .map(|text| (text, fresh_request.contains(text)));
    assert_eq!(
        rules,
        [
            (
                "please never run git commit or anything that rewrites history.",
                true
            ),
            (
                "don't run the whole test suite every time - just run the test file(s) relevant to what you changed.",
                true
            ),
            ("The test env is the venv one level up (../venv).", true),
            (
                "end each of your replies with a single line starting with 'Next:' that says the one concrete next step you'd take.",
                true
            ),
            ("No code changes yet", false),
            ("Prefer concise progress updates", false),
        ]
    );
    assert!(fresh_request.contains(
        "User rules (the user's exact words; they apply to all work in this project until the user changes them):"
    ));
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
