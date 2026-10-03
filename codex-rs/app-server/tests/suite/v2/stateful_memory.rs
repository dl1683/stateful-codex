//! Visible, correctable memory (council slice 5): the user reviews project memory, forgets
//! one rule and corrects another with no model turn, and a fresh thread follows the result.

use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_app_server_protocol::StatefulMemorySection;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const SUITE_RULE: &str = "From now on, never run the whole test suite.";
const NEXT_RULE: &str = "From now on, end every reply with a line starting with 'Next:'.";
const CORRECTED: &str = "End every reply with a line starting with 'Next step:'.";

#[tokio::test]
async fn the_user_reviews_forgets_and_corrects_memory_without_a_model_turn() -> Result<()> {
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
                name: "Memory".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "memory-project".to_string(),
            },
        })
        .await?;
    let log = responses::mount_sse_sequence(
        &responses_server,
        (0..2)
            .map(|_| {
                responses::sse(vec![
                    responses::ev_assistant_message("assistant-message", "Done."),
                    responses::ev_completed("assistant-response"),
                ])
            })
            .collect(),
    )
    .await;
    let first = start_thread(&mut server, &project.project.id).await?;
    run_turn(&mut server, &first, &format!("{SUITE_RULE} {NEXT_RULE}")).await?;

    let read = |thread_id: String| {
        move |request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id,
                cursor: None,
                limit: None,
            },
        }
    };
    let before: StatefulMemoryReadResponse = server.request(read(first.clone())).await?;
    let listed = before
        .data
        .iter()
        .map(|item| (item.section, item.kind, item.source, item.content.clone()))
        .collect::<Vec<_>>();
    let mut expected = [SUITE_RULE, NEXT_RULE].map(|content| {
        (
            StatefulMemorySection::UserRule,
            BlackboardKind::Instruction,
            BlackboardProvenanceKind::User,
            content.to_string(),
        )
    });
    expected.sort_by(|left, right| {
        let position = |content: &str| listed.iter().position(|item| item.3 == content);
        position(&left.3).cmp(&position(&right.3))
    });
    assert_eq!(
        (
            before.project_id.clone(),
            listed,
            before.next_cursor.clone()
        ),
        (project.project.id.clone(), expected.to_vec(), None)
    );
    let item = |content: &str| {
        before
            .data
            .iter()
            .find(|item| item.content == content)
            .cloned()
            .expect("listed")
    };
    let (suite, next) = (item(SUITE_RULE), item(NEXT_RULE));
    let page = |thread_id: String, cursor: Option<String>| {
        move |request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id,
                cursor,
                limit: Some(1),
            },
        }
    };
    let first_page: StatefulMemoryReadResponse = server.request(page(first.clone(), None)).await?;

    let forgotten: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                thread_id: first.clone(),
                entry_id: suite.entry_id.clone(),
                expected_revision: suite.revision,
            },
        })
        .await?;
    let corrected: StatefulMemoryCorrectResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryCorrect {
            request_id,
            params: StatefulMemoryCorrectParams {
                thread_id: first.clone(),
                entry_id: next.entry_id.clone(),
                expected_revision: next.revision,
                content: CORRECTED.to_string(),
            },
        })
        .await?;
    // A cursor from before the changes is refused instead of skipping an entry.
    let stale_id = server
        .send_request(
            "statefulMemory/read",
            Some(serde_json::json!({
                "threadId": first,
                "cursor": first_page.next_cursor,
                "limit": 1
            })),
        )
        .await?;
    let stale = server
        .read_stream_until_error_message(codex_app_server_protocol::RequestId::Integer(stale_id))
        .await?;
    assert_eq!(
        stale.error.message,
        "project memory changed since this page was read; read it again from the start"
    );
    // Browsing and correcting made no model request.
    assert_eq!(log.requests().len(), 1);
    assert_eq!(
        (
            forgotten.entry_id,
            corrected.item.section,
            corrected.item.content.clone(),
            corrected
                .item
                .replaces
                .iter()
                .map(|replaced| replaced.content.clone())
                .collect::<Vec<_>>(),
        ),
        (
            suite.entry_id.clone(),
            StatefulMemorySection::UserRule,
            CORRECTED.to_string(),
            vec![NEXT_RULE.to_string()],
        )
    );
    let after: StatefulMemoryReadResponse = server.request(read(first.clone())).await?;
    assert_eq!(
        after
            .data
            .iter()
            .map(|item| item.content.clone())
            .collect::<Vec<_>>(),
        vec![CORRECTED.to_string()]
    );

    // A fresh thread's packet carries the corrected rule; the forgotten one is gone and the
    // old wording appears only as what the correction replaced.
    let fresh = start_thread(&mut server, &project.project.id).await?;
    run_turn(&mut server, &fresh, "Rename the helper in utils.py").await?;
    let packet = log.requests()[1].body_json().to_string();
    assert_eq!(
        (
            packet.contains(SUITE_RULE),
            packet.contains(CORRECTED),
            packet.matches(NEXT_RULE).count(),
            packet.contains(&format!("replaces: \\\"{NEXT_RULE}\\\"")),
        ),
        (false, true, 1, true)
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
