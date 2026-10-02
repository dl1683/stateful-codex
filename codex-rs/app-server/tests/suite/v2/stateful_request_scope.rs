//! The cheap ordinary path: a self-contained request defers the conversation record and
//! carries a scope note; a later request that refers to earlier work gets the record.

use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ThreadCompactStartParams;
use codex_app_server_protocol::ThreadCompactStartResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const EARLIER_MARKER: &str = "EARLIER_THREAD_PROPOSAL_MARKER";
const NARROW_REQUEST: &str = "Rename the helper in utils.py to snake_case and update its callers";
const SELF_CONTAINED_NOTE: &str =
    "Current request scope (applies until a later scope note): self-contained.";
const CONTINUITY_NOTE: &str = "Current request scope (applies until a later scope note): this request may depend on earlier work";

#[tokio::test]
async fn self_contained_requests_defer_the_record_until_a_request_refers_to_earlier_work()
-> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Request scope".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "request-scope-project".to_string(),
            },
        })
        .await?;
    let earlier = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;
    send_turn(&mut server, &earlier.thread.id, EARLIER_MARKER).await?;

    let fresh = server
        .start_thread(ThreadStartParams {
            project_id: Some(created.project.id.clone()),
            ..Default::default()
        })
        .await?;
    // Two narrow turns, a manual compaction, another narrow turn, then a reply to the
    // earlier proposal.
    send_turn(&mut server, &fresh.thread.id, NARROW_REQUEST).await?;
    send_turn(&mut server, &fresh.thread.id, NARROW_REQUEST).await?;
    let compact_request = server
        .send_thread_compact_start_request(ThreadCompactStartParams {
            thread_id: fresh.thread.id.clone(),
        })
        .await?;
    let _: ThreadCompactStartResponse = server.read_response(compact_request).await?;
    let _: codex_app_server_protocol::TurnCompletedNotification =
        server.read_notification("turn/completed").await?;
    send_turn(&mut server, &fresh.thread.id, NARROW_REQUEST).await?;
    send_turn(&mut server, &fresh.thread.id, "yes, as proposed").await?;

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
    let [
        ..,
        first_narrow,
        second_narrow,
        _compaction,
        after_compaction,
        reply,
    ] = bodies.as_slice()
    else {
        panic!("every fresh-thread request should be recorded");
    };
    let observed = [first_narrow, second_narrow, after_compaction, reply]
        .iter()
        .map(|body| {
            (
                body.contains("<stateful_project>"),
                body.matches("<stateful_continuity>").count(),
                body.contains(EARLIER_MARKER),
                body.matches(SELF_CONTAINED_NOTE).count(),
                body.matches(CONTINUITY_NOTE).count(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        vec![
            // The narrow request keeps the project packet (rules and knowledge), defers the
            // record, and carries the scope note once.
            (true, 0, false, 1, 0),
            // A second narrow turn adds nothing.
            (true, 0, false, 1, 0),
            // After compaction the note returns once with the new window.
            (true, 0, false, 1, 0),
            // A reply to the earlier proposal gets the record and lifts the restriction.
            (true, 1, true, 1, 1),
        ]
    );
    Ok(())
}

async fn send_turn(server: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
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
