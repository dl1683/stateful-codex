//! Investigation-scoped rules (build item 1): a rule for a whole investigation applies in the
//! thread that opened it and in a thread that continues it, never in unrelated work, and stops
//! applying when the user ends the investigation.

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
use tempfile::TempDir;

/// debug2's opening.
const OPENING: &str = "I need help chasing a bug in shipit.\n\nPlease investigate and find the root cause. Some ground rules for this whole investigation, which may take a few days:\n- Do NOT change any code until we have agreed on the root cause. Reading and running things are fine.\n- Keep a running list of the hypotheses you have ruled out.\n\nTell me what you found.";
const STORED: &str =
    "which may take a few days: Do NOT change any code until we have agreed on the root cause.";
const NOT_HERE: &str = "belong to an investigation this thread is not part of";
const CONTINUES: &str = "This thread continues the investigation";

#[tokio::test]
async fn investigation_rules_apply_only_while_and_where_the_investigation_runs() -> Result<()> {
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
                name: "Scope".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "scope-project".to_string(),
            },
        })
        .await?;
    let log = responses::mount_sse_sequence(
        &responses_server,
        (0..5)
            .map(|_| {
                responses::sse(vec![
                    responses::ev_assistant_message("assistant-message", "Done."),
                    responses::ev_completed("assistant-response"),
                ])
            })
            .collect(),
    )
    .await;
    let opened = start_thread(&mut server, &project.project.id).await?;
    run_turn(&mut server, &opened, OPENING).await?;
    // Unrelated work in a new thread.
    let unrelated = start_thread(&mut server, &project.project.id).await?;
    run_turn(
        &mut server,
        &unrelated,
        "Rename the helper in utils.py to snake_case and update its callers",
    )
    .await?;
    // A new thread that continues the earlier work joins the one open investigation.
    let continued = start_thread(&mut server, &project.project.id).await?;
    run_turn(
        &mut server,
        &continued,
        "It's been a few days. Remind me what we ruled out so far.",
    )
    .await?;
    run_turn(
        &mut server,
        &continued,
        "Great, we've agreed on the root cause. Let's plan the fix.",
    )
    .await?;
    run_turn(&mut server, &continued, "Go on with the plan.").await?;
    let requests = log.requests();
    let packet = |index: usize| requests[index].body_json().to_string();
    // The rule as the root shows it, with its investigation (the raw message also reaches
    // later threads through the conversation record, so the raw words prove nothing).
    let applied = |index: usize| packet(index).contains(STORED);
    let elsewhere = |index: usize| packet(index).contains(NOT_HERE);
    let continues = |index: usize| packet(index).contains(CONTINUES);
    assert_eq!(
        [
            (applied(0), continues(0), elsewhere(0)),
            (applied(1), continues(1), elsewhere(1)),
            (applied(2), continues(2), elsewhere(2)),
            (applied(4), continues(4), elsewhere(4)),
        ],
        [
            (true, true, false),
            (false, false, true),
            (true, true, false),
            // After the user ended it, the thread is told its rules no longer apply.
            (true, true, true),
        ]
    );
    Ok(())
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
