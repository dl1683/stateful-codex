//! C4-C6 repair 1-2 boundaries through the public app-server: multipart messages are never
//! admitted; Apply settles only a whole cited message with no unresolved scope; receipts mark
//! shortened text; a recorded Apply replays truthfully after Forget, Undo or restart.
use super::*;
use codex_app_server_protocol::StatefulMemoryApplyCategory;
use codex_app_server_protocol::StatefulMemoryApplyParams;
use codex_app_server_protocol::StatefulMemoryApplyResponse;
use codex_app_server_protocol::StatefulMemoryCapturedNotification;
use codex_app_server_protocol::StatefulMemoryReceipt;
use codex_app_server_protocol::StatefulMemoryReceiptKind;
use codex_app_server_protocol::StatefulMemoryUndoParams;
use codex_app_server_protocol::StatefulMemoryUndoResponse;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use wiremock::Mock;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

pub(super) const DECISION: &str =
    "We'll use SQLite rather than Postgres, because the tool must run offline on a laptop.";

pub(super) type Script = Box<dyn Fn(usize, &Value) -> String + Send + Sync>;

pub(super) fn message(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("answer", text),
        responses::ev_completed("answer"),
    ])
}

pub(super) fn call(id: &str, tool: &str, args: Value) -> String {
    responses::sse(vec![
        responses::ev_function_call(id, tool, &args.to_string()),
        responses::ev_completed(id),
    ])
}

/// A same-turn proposal citing `[0, end)` of the newest sealed part (`None`: the whole part).
pub(super) fn proposal(
    body: &Value,
    category: &str,
    interpretation: &str,
    end: Option<u64>,
    dependency: Option<&str>,
) -> String {
    let handles = super::source_proposals_tests::source_handles(body)
        .expect("host issued source handles in the normal request");
    let handle = &handles["handles"][0];
    let mut record = json!({
        "sourceId":handle[0],"digest":handle[1],"sourceRevision":handle[2],"partIndex":handle[3],
        "spans":[{"startByte":0,"endByte":end.map_or(handle[4].clone(), Value::from),"role":"body"}],
        "category":category,"interpretation":interpretation
    });
    if let Some(dependency) = dependency {
        record["dependency"] = json!(dependency);
    }
    call(
        "proposal",
        "blackboard_record_batch",
        json!({"type":"sourceProposal","records":[record]}),
    )
}

pub(super) async fn mount(server: &wiremock::MockServer, script: Script) -> Arc<Mutex<Vec<Value>>> {
    let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = calls.clone();
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).expect("request body is JSON");
            let mut calls = captured.lock().expect("request log lock");
            let response = script(calls.len(), &body);
            calls.push(body);
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(response)
        })
        .mount(server)
        .await;
    calls
}

pub(super) async fn turn(server: &mut TestAppServer, thread: &str, parts: &[&str]) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.to_string(),
            input: parts
                .iter()
                .map(|text| UserInput::Text {
                    text: (*text).to_string(),
                    text_elements: Vec::new(),
                })
                .collect(),
            ..Default::default()
        })
        .await?;
    Ok(())
}

pub(super) async fn kept(server: &mut TestAppServer) -> Result<StatefulMemoryReceipt> {
    let notification = server
        .read_stream_until_matching_notification("turn-driven memory receipt", |notification| {
            notification.method == "statefulMemory/captured"
                && notification.params.as_ref().is_some_and(|params| {
                    serde_json::from_value::<StatefulMemoryCapturedNotification>(params.clone())
                        .is_ok_and(|captured| {
                            matches!(
                                captured.receipt.kind,
                                StatefulMemoryReceiptKind::Rules
                                    | StatefulMemoryReceiptKind::Proposals
                            )
                        })
                })
        })
        .await?;
    let params = notification.params.expect("receipt params");
    Ok(serde_json::from_value::<StatefulMemoryCapturedNotification>(params)?.receipt)
}

pub(super) fn apply_request(
    project: &str,
    thread: &str,
    entry_id: &str,
    category: StatefulMemoryApplyCategory,
    action: &str,
) -> impl FnOnce(codex_app_server_protocol::RequestId) -> ClientRequest {
    let (project, thread, entry_id, action) = (
        project.to_string(),
        thread.to_string(),
        entry_id.to_string(),
        action.to_string(),
    );
    move |request_id| ClientRequest::StatefulMemoryApply {
        request_id,
        params: StatefulMemoryApplyParams {
            thread_id: thread,
            expected_project_id: project,
            entry_id,
            expected_revision: 1,
            category,
            client_action_id: action,
        },
    }
}

/// The JSON-RPC error message of a request that must fail.
async fn refused(
    server: &mut TestAppServer,
    request: impl FnOnce(codex_app_server_protocol::RequestId) -> ClientRequest,
) -> Result<String> {
    let request = request(codex_app_server_protocol::RequestId::Integer(0));
    let value = serde_json::to_value(&request)?;
    let id = server
        .send_request(
            value["method"].as_str().expect("method"),
            Some(value["params"].clone()),
        )
        .await?;
    let error = server
        .read_stream_until_error_message(codex_app_server_protocol::RequestId::Integer(id))
        .await?;
    Ok(error.error.message)
}

async fn restart(mut server: TestAppServer, home: &TempDir) -> Result<TestAppServer> {
    assert!(server.shutdown_gracefully().await?.success());
    drop(server);
    TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await
}

pub(super) async fn root_contents(home: &TempDir, project: &str) -> Result<Vec<String>> {
    let store = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs())).await?;
    Ok(store
        .root_projection(RootBlackboardQuery {
            project_id: project.to_string(),
            max_entries: 256,
        })
        .await?
        .data
        .into_iter()
        .map(|hit| hit.entry.value.content)
        .collect())
}

#[tokio::test]
async fn c456r1_multipart_messages_are_never_admitted_as_rules() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let _calls = mount(&responses_server, Box::new(|_, _| message("Done."))).await;
    let rules = "Ground rules for this project:\n- Never push.";
    for parts in [
        vec!["Priya wrote the following:", rules],
        vec!["Priya wrote the following; do not adopt it:", rules],
        vec![rules, "These are for today's task only."],
    ] {
        turn(&mut server, &thread, &parts).await?;
    }
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    // The exact parts stay sealed history; nothing was admitted or receipted as Saved.
    assert!(
        !server
            .pending_notification_methods()
            .iter()
            .any(|method| method == "statefulMemory/captured")
    );
    let mut server = restart(server, &home).await?;
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    // One-part positive control.
    let second = start_thread(&mut server, &project).await?;
    turn(&mut server, &second, &[rules]).await?;
    assert_eq!(
        root_contents(&home, &project).await?,
        vec!["Never push.".to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn c456r3_declarations_followed_by_any_text_are_never_admitted() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let _calls = mount(&responses_server, Box::new(|_, _| message("Done."))).await;
    for text in [
        "Ground rules for this project:\n- Never push.\n\nFor today, the above rule applies only to today's task; it is not a standing rule.",
        "Ground rules for this project:\n- Never push.\n\nFirst task: investigate this.\nThe rules above are Priya's quoted policy, not my instructions to you.",
        "Ground rules for this project:\n- Never push.\n\nFor today, inspect the tests.",
    ] {
        turn(&mut server, &thread, &[text]).await?;
    }
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    // Nothing was admitted or receipted as Saved.
    assert!(
        !server
            .pending_notification_methods()
            .iter()
            .any(|method| method == "statefulMemory/captured")
    );
    // A cold new process and thread start with the same empty root.
    let mut server = restart(server, &home).await?;
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    // Positive control: the bare declaration is still saved.
    let second = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &second,
        &["Ground rules for this project:\n- Never push."],
    )
    .await?;
    assert_eq!(
        root_contents(&home, &project).await?,
        vec!["Never push.".to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn c456r1_apply_settles_only_whole_unscoped_messages() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let reason_starts = DECISION.find(", because").expect("reason") as u64;
    let _calls = mount(
        &responses_server,
        Box::new(move |sequence, body| match sequence {
            // The citation drops the reason while the reading includes it.
            0 => proposal(
                body,
                "decision",
                "SQLite over Postgres because it must work offline on a laptop.",
                Some(reason_starts),
                None,
            ),
            2 => proposal(
                body,
                "decision",
                "SQLite for this task only.",
                None,
                Some("scope"),
            ),
            4 => proposal(
                body,
                "ruledOut",
                "The cache is ruled out for this task only.",
                None,
                Some("scope"),
            ),
            _ => message("Kept."),
        }),
    )
    .await;
    let mut ids = Vec::new();
    for text in [
        DECISION,
        "For this task only, we'll use SQLite rather than Postgres.",
        "For this task only, it's not the cache - clearing it didn't change the timing.",
    ] {
        turn(&mut server, &thread, &[text]).await?;
        let receipt = kept(&mut server).await?;
        // No Apply affordance: there are no exact words an Apply could settle.
        assert_eq!(
            (
                receipt.members[0].applies_text.clone(),
                receipt.members[0].applies_text_shortened
            ),
            (None, false)
        );
        ids.push((
            receipt.members[0].entry_id.clone().expect("kept entry"),
            receipt.members[0].category,
        ));
    }
    for (index, (id, _)) in ids.iter().enumerate() {
        let category = if index == 2 {
            StatefulMemoryApplyCategory::RuledOut
        } else {
            StatefulMemoryApplyCategory::Decision
        };
        let message = refused(
            &mut server,
            apply_request(&project, &thread, id, category, &format!("apply-{index}")),
        )
        .await?;
        assert!(message.contains("nothing to apply"), "{message}");
    }
    let mut server = restart(server, &home).await?;
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    let unrelated = start_thread(&mut server, &project).await?;
    turn(&mut server, &unrelated, &["Start the unrelated task."]).await?;
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn c456r1_apply_replay_after_forget_refuses_without_announcing_application() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let _calls = mount(
        &responses_server,
        Box::new(|sequence, body| match sequence {
            0 => proposal(body, "decision", "SQLite for offline laptops.", None, None),
            _ => message("Noted."),
        }),
    )
    .await;
    turn(&mut server, &thread, &[DECISION]).await?;
    let id = kept(&mut server).await?.members[0]
        .entry_id
        .clone()
        .expect("kept entry");
    let applied: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-once",
        ))
        .await?;
    let revision = applied.receipt.members[0]
        .revision
        .expect("applied revision");
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                entry_id: id.clone(),
                expected_revision: revision,
            },
        })
        .await?;
    let mut server = restart(server, &home).await?;
    server.clear_message_buffer();
    let message = refused(
        &mut server,
        apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-once",
        ),
    )
    .await?;
    assert!(message.contains("nothing was restored"), "{message}");
    assert!(
        !server
            .pending_notification_methods()
            .iter()
            .any(|method| method == "statefulMemory/captured" || method == "blackboard/updated")
    );
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    Ok(())
}

/// One whole message of exactly `bytes` bytes, ending in a material qualifier, with a
/// two-byte character at the boundary when the length allows.
fn sized_decision(bytes: usize) -> String {
    let mut decision = "We'll keep the ledger format, because the auditors parse it: ".to_string();
    while decision.len() + 2 <= bytes - "only for exports.".len() {
        decision.push('é');
    }
    while decision.len() < bytes - "only for exports.".len() {
        decision.push('x');
    }
    decision.push_str("only for exports.");
    assert_eq!(decision.len(), bytes);
    decision
}

#[tokio::test]
async fn c456r2_apply_takes_whole_words_up_to_240_bytes_and_receipts_stay_exact() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let fits = sized_decision(240);
    let over = sized_decision(241);
    let calls = mount(
        &responses_server,
        Box::new(|sequence, body| match sequence {
            1 => proposal(
                body,
                "decision",
                "Keep the ledger format for auditors.",
                None,
                None,
            ),
            3 => proposal(
                body,
                "decision",
                "Keep the ledger format, exports only.",
                None,
                None,
            ),
            _ => message("Noted."),
        }),
    )
    .await;
    // A long accepted rule: its receipt preview is marked shortened and /memory holds it whole.
    let path = format!("vendor/{}/", "deep".repeat(60));
    let rule = format!("Don't touch {path} or any changelog.");
    turn(
        &mut server,
        &thread,
        &[&format!("Ground rules for this project:\n- {rule}")],
    )
    .await?;
    let rules = kept(&mut server).await?;
    assert_eq!(
        (
            rules.members[0].text_shortened,
            rules.members[0].text.len() <= 240,
            rule.starts_with(rules.members[0].text.as_str())
        ),
        (true, true, true)
    );
    // 240 bytes: the receipt shows the whole prospective quotation and Apply settles it.
    turn(&mut server, &thread, &[&fits]).await?;
    let receipt = kept(&mut server).await?;
    assert_eq!(
        (
            receipt.members[0].applies_text.as_deref(),
            receipt.members[0].applies_text_shortened
        ),
        (Some(fits.as_str()), false)
    );
    let id = receipt.members[0].entry_id.clone().expect("kept entry");
    let applied: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-fits",
        ))
        .await?;
    assert_eq!(
        (
            applied.receipt.members[0].text.as_str(),
            applied.receipt.members[0].text_shortened
        ),
        (fits.as_str(), false)
    );
    // 241 bytes: no Apply is offered and an Apply is refused.
    turn(&mut server, &thread, &[&over]).await?;
    let receipt = kept(&mut server).await?;
    assert_eq!(
        (
            receipt.members[0].applies_text.clone(),
            receipt.members[0].applies_text_shortened
        ),
        (None, false)
    );
    let message = refused(
        &mut server,
        apply_request(
            &project,
            &thread,
            receipt.members[0].entry_id.as_deref().expect("kept entry"),
            StatefulMemoryApplyCategory::Decision,
            "apply-over",
        ),
    )
    .await?;
    assert!(message.contains("at most 240 bytes"), "{message}");
    // Restart: the whole applied words reach /memory review and a cold new thread's root.
    let mut server = restart(server, &home).await?;
    let replayed: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-fits",
        ))
        .await?;
    assert_eq!(replayed.receipt, applied.receipt);
    let listed: StatefulMemoryReadResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await?;
    let contents = listed
        .data
        .iter()
        .map(|item| (item.content.clone(), item.content_truncated))
        .collect::<Vec<_>>();
    assert!(contents.contains(&(fits.clone(), false)));
    assert!(contents.contains(&(rule.clone(), false)));
    let cold = start_thread(&mut server, &project).await?;
    turn(&mut server, &cold, &["Continue."]).await?;
    let request = calls
        .lock()
        .expect("request log lock")
        .last()
        .cloned()
        .expect("request");
    let request = request.to_string();
    assert!(
        request.contains(
            &serde_json::to_string(&format!("content={fits}"))?
                .trim_matches('"')
                .to_string()
        )
    );
    // After Undo the same action's retry refuses, before and after restart, changing nothing.
    let undo: StatefulMemoryUndoResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryUndo {
            request_id,
            params: StatefulMemoryUndoParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                receipt_id: applied.receipt.receipt_id.clone(),
                client_action_id: "undo-fits".to_string(),
            },
        })
        .await?;
    assert_eq!(undo.receipt.kind, StatefulMemoryReceiptKind::Undo);
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let before = super::model_retirement_tests::snapshot(&sqlite).await?;
    for restart_first in [false, true] {
        if restart_first {
            server = restart(server, &home).await?;
        }
        let message = refused(
            &mut server,
            apply_request(
                &project,
                &thread,
                &id,
                StatefulMemoryApplyCategory::Decision,
                "apply-fits",
            ),
        )
        .await?;
        assert!(message.contains("nothing was restored"), "{message}");
    }
    assert_eq!(
        super::model_retirement_tests::snapshot(&sqlite).await?,
        before
    );
    assert!(!root_contents(&home, &project).await?.contains(&fits));
    Ok(())
}

#[tokio::test]
async fn c456r2_apply_replay_refuses_after_a_sibling_forgets_the_shared_source() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let _calls = mount(
        &responses_server,
        Box::new(|sequence, body| match sequence {
            0 => {
                let handles = super::source_proposals_tests::source_handles(body)
                    .expect("host issued source handles");
                let handle = &handles["handles"][0];
                let record = |category: &str, interpretation: &str| {
                    json!({
                        "sourceId":handle[0],"digest":handle[1],"sourceRevision":handle[2],"partIndex":handle[3],
                        "spans":[{"startByte":0,"endByte":handle[4],"role":"body"}],
                        "category":category,"interpretation":interpretation
                    })
                };
                call(
                    "proposal",
                    "blackboard_record_batch",
                    json!({"type":"sourceProposal","records":[
                        record("decision", "SQLite for offline laptops."),
                        record("note", "Offline laptops are a hard requirement."),
                    ]}),
                )
            }
            _ => message("Noted."),
        }),
    )
    .await;
    turn(&mut server, &thread, &[DECISION]).await?;
    let receipt = kept(&mut server).await?;
    let decision = receipt.members[0].entry_id.clone().expect("decision");
    let note = receipt.members[1].entry_id.clone().expect("note");
    let applied: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &decision,
            StatefulMemoryApplyCategory::Decision,
            "apply-shared",
        ))
        .await?;
    assert_eq!(applied.receipt.members[0].text, DECISION);
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                entry_id: note.clone(),
                expected_revision: 1,
            },
        })
        .await?;
    // The shared source is fenced: the applied decision no longer reaches the root.
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    let mut server = restart(server, &home).await?;
    server.clear_message_buffer();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let before = super::model_retirement_tests::snapshot(&sqlite).await?;
    let message = refused(
        &mut server,
        apply_request(
            &project,
            &thread,
            &decision,
            StatefulMemoryApplyCategory::Decision,
            "apply-shared",
        ),
    )
    .await?;
    assert!(message.contains("nothing was restored"), "{message}");
    assert!(
        !server
            .pending_notification_methods()
            .iter()
            .any(|method| method == "statefulMemory/captured" || method == "blackboard/updated")
    );
    assert_eq!(
        super::model_retirement_tests::snapshot(&sqlite).await?,
        before
    );
    assert_eq!(root_contents(&home, &project).await?, Vec::<String>::new());
    Ok(())
}
