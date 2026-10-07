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
use codex_app_server_protocol::StatefulMemoryAddKind;
use codex_app_server_protocol::StatefulMemoryAddOutcome;
use codex_app_server_protocol::StatefulMemoryAddParams;
use codex_app_server_protocol::StatefulMemoryAddResponse;
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
                background_section: false,
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
                background_section: false,
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
                background_section: false,
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

/// What the user says about themselves is reviewed under its own section by clients that
/// know it and as knowledge by older ones; it stays background through corrections, and a
/// forgotten statement leaves the next fresh packet.
#[tokio::test]
async fn background_is_reviewed_corrected_and_forgotten_as_background() -> Result<()> {
    const BACKGROUND: &str = "I'm a backend developer, mostly Go for the last six years.";
    const CORRECTED_BACKGROUND: &str =
        "I'm a backend developer, mostly Go, and new to Python packaging.";
    const AGAIN: &str = "I'm a backend developer, mostly Go, and I know Python packaging now.";
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
                name: "Background".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "background-project".to_string(),
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
    let thread = start_thread(&mut server, &project.project.id).await?;
    let added: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Background,
                content: BACKGROUND.to_string(),
                scope: None,
                reason: None,
                client_action_id: "explicit-background".to_string(),
                background_section: true,
            },
        })
        .await?;
    assert_eq!(
        (added.item.content, added.item.authority),
        (
            BACKGROUND.to_string(),
            Some(codex_app_server_protocol::StatefulMemoryAuthority::HumanDirect)
        )
    );
    run_turn(&mut server, &thread, "Review the project.").await?;
    assert!(server.shutdown_gracefully().await?.success());
    server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let request_id = server
        .send_thread_resume_request(codex_app_server_protocol::ThreadResumeParams {
            thread_id: thread.clone(),
            ..Default::default()
        })
        .await?;
    let _: codex_app_server_protocol::ThreadResumeResponse =
        server.read_response(request_id).await?;
    let read = |thread_id: String, background_section: bool| {
        move |request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id,
                cursor: None,
                limit: None,
                background_section,
            },
        }
    };
    let current: StatefulMemoryReadResponse = server.request(read(thread.clone(), true)).await?;
    let legacy: StatefulMemoryReadResponse = server.request(read(thread.clone(), false)).await?;
    let item = current.data[0].clone();
    let correct = |entry_id: String, expected_revision: u64, content: &str| {
        let thread_id = thread.clone();
        let content = content.to_string();
        move |request_id| ClientRequest::StatefulMemoryCorrect {
            request_id,
            params: StatefulMemoryCorrectParams {
                thread_id,
                entry_id,
                expected_revision,
                content,
                background_section: true,
            },
        }
    };
    let corrected: StatefulMemoryCorrectResponse = server
        .request(correct(
            item.entry_id.clone(),
            item.revision,
            CORRECTED_BACKGROUND,
        ))
        .await?;
    let again: StatefulMemoryCorrectResponse = server
        .request(correct(
            corrected.item.entry_id.clone(),
            corrected.item.revision,
            AGAIN,
        ))
        .await?;
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                thread_id: thread.clone(),
                entry_id: again.item.entry_id.clone(),
                expected_revision: again.item.revision,
            },
        })
        .await?;
    let fresh = start_thread(&mut server, &project.project.id).await?;
    run_turn(&mut server, &fresh, "Rename the helper in utils.py").await?;
    let packet = log.requests()[1].body_json().to_string();
    assert_eq!(
        (
            (item.section, item.content.clone()),
            legacy.data[0].section,
            (corrected.item.section, corrected.item.content),
            again.item.section,
            packet.contains("backend developer"),
        ),
        (
            (StatefulMemorySection::Background, BACKGROUND.to_string()),
            StatefulMemorySection::Knowledge,
            (
                StatefulMemorySection::Background,
                CORRECTED_BACKGROUND.to_string()
            ),
            StatefulMemorySection::Background,
            false,
        )
    );
    Ok(())
}

/// The user adds a rule, something about themselves and a decision with its reason before
/// any turn, with no model turn; a retried action changes nothing; a fresh thread applies
/// the rule and shows the background.
#[tokio::test]
async fn the_user_adds_to_memory_without_a_model_turn() -> Result<()> {
    const RULE: &str = "End every reply with a line starting with 'Next:'.";
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
                name: "Add".to_string(),
                roots: Vec::new(),
                metadata: Some(BTreeMap::new()),
                idempotency_key: "add-project".to_string(),
            },
        })
        .await?;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![responses::sse(vec![
            responses::ev_assistant_message("assistant-message", "Done.\nNext: test it."),
            responses::ev_completed("assistant-response"),
        ])],
    )
    .await;
    let thread = start_thread(&mut server, &project.project.id).await?;
    let add = |kind: StatefulMemoryAddKind, content: &str, reason: Option<&str>, action: &str| {
        let params = StatefulMemoryAddParams {
            thread_id: thread.clone(),
            kind,
            content: content.to_string(),
            scope: None,
            reason: reason.map(str::to_string),
            client_action_id: action.to_string(),
            background_section: true,
        };
        move |request_id| ClientRequest::StatefulMemoryAdd { request_id, params }
    };
    let mut outcomes = Vec::new();
    for (kind, content, reason, action) in [
        (StatefulMemoryAddKind::Rule, RULE, None, "add-1"),
        (StatefulMemoryAddKind::Rule, RULE, None, "add-1"),
        (
            StatefulMemoryAddKind::Background,
            "I'm a backend developer, mostly Go.",
            None,
            "add-2",
        ),
        (
            StatefulMemoryAddKind::Decision,
            "Months use the symbol mth.",
            Some("dashboard readers confused mo with minutes"),
            "add-3",
        ),
    ] {
        let response: StatefulMemoryAddResponse =
            server.request(add(kind, content, reason, action)).await?;
        outcomes.push((
            response.item.section,
            response.item.content,
            response.outcome,
        ));
    }
    let fresh = start_thread(&mut server, &project.project.id).await?;
    run_turn(&mut server, &fresh, "Rename the helper in utils.py").await?;
    let packet = log.single_request().body_json().to_string();
    assert_eq!(
        (
            outcomes,
            packet.contains(RULE),
            packet.contains("mostly Go"),
            packet.contains("confused mo with minutes"),
        ),
        (
            vec![
                (
                    StatefulMemorySection::UserRule,
                    RULE.to_string(),
                    StatefulMemoryAddOutcome::Added
                ),
                (
                    StatefulMemorySection::UserRule,
                    RULE.to_string(),
                    StatefulMemoryAddOutcome::AlreadyDone
                ),
                (
                    StatefulMemorySection::Background,
                    "I'm a backend developer, mostly Go.".to_string(),
                    StatefulMemoryAddOutcome::Added
                ),
                (
                    StatefulMemorySection::Decision,
                    "Months use the symbol mth. Reason: dashboard readers confused mo with minutes"
                        .to_string(),
                    StatefulMemoryAddOutcome::Added
                ),
            ],
            true,
            true,
            true,
        )
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

#[tokio::test]
async fn public_noop_add_retry_after_forget_and_restart_does_not_restore_words() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Retries".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "retry-project".to_string(),
            },
        })
        .await?;
    let thread = start_thread(&mut server, &project.project.id).await?;
    responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("restart-turn"),
            responses::ev_completed("restart-turn"),
        ]),
    )
    .await;
    run_turn(
        &mut server,
        &thread,
        "Persist this conversation for a restart.",
    )
    .await?;
    let add = |action: &str| {
        let thread = thread.clone();
        let action = action.to_string();
        move |request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                thread_id: thread,
                kind: StatefulMemoryAddKind::Rule,
                content: "Never push.".to_string(),
                scope: None,
                reason: None,
                client_action_id: action,
                background_section: true,
            },
        }
    };
    let first: StatefulMemoryAddResponse = server.request(add("first")).await?;
    let noop: StatefulMemoryAddResponse = server.request(add("noop")).await?;
    assert_eq!(
        (first.outcome, noop.outcome, first.item.entry_id.clone()),
        (
            codex_app_server_protocol::StatefulMemoryAddOutcome::Added,
            codex_app_server_protocol::StatefulMemoryAddOutcome::AlreadyPresent,
            noop.item.entry_id.clone()
        )
    );
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                thread_id: thread.clone(),
                entry_id: first.item.entry_id.clone(),
                expected_revision: first.item.revision,
            },
        })
        .await?;
    assert!(server.shutdown_gracefully().await?.success());
    server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let request_id = server
        .send_thread_resume_request(codex_app_server_protocol::ThreadResumeParams {
            thread_id: thread.clone(),
            ..Default::default()
        })
        .await?;
    let _: codex_app_server_protocol::ThreadResumeResponse =
        server.read_response(request_id).await?;
    let retry: StatefulMemoryAddResponse = server.request(add("noop")).await?;
    let memory: StatefulMemoryReadResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: thread.clone(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await?;
    assert_eq!(
        (retry.outcome, retry.item.entry_id, memory.data),
        (
            codex_app_server_protocol::StatefulMemoryAddOutcome::AlreadyDone,
            first.item.entry_id,
            Vec::new()
        )
    );
    Ok(())
}
