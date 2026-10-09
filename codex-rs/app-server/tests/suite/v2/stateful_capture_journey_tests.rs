//! C4-C6 capture-to-use journey through the public app-server: two stated rules, a
//! conversation-only decision and a rejected hypothesis in one session; a cold new session
//! applies and recalls them; Forget removes a rule for good, across restarts and restatement.
use super::*;
use codex_app_server_protocol::StatefulMemoryApplyCategory;
use codex_app_server_protocol::StatefulMemoryApplyParams;
use codex_app_server_protocol::StatefulMemoryApplyResponse;
use codex_app_server_protocol::StatefulMemoryCapturedNotification;
use codex_app_server_protocol::StatefulMemoryReceiptCategory;
use codex_app_server_protocol::StatefulMemoryReceiptKind;
use codex_app_server_protocol::StatefulMemoryReceiptStatus;
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

const NEXT_RULE: &str = "Always end each reply with Next:";
const VENDOR_RULE: &str = "Don't touch vendor/ or any changelog.";
const DECISION: &str =
    "We'll use SQLite rather than Postgres, because the tool must run offline on a laptop.";
const RULED_OUT: &str = "It's not the cache - clearing it didn't change the timing.";

fn declaration() -> String {
    format!("Ground rules for this project:\n- {NEXT_RULE}\n- {VENDOR_RULE}")
}

fn message(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("answer", text),
        responses::ev_completed("answer"),
    ])
}

/// The model's ordinary same-turn proposal of the user's whole sentence.
fn proposal(body: &Value, category: &str, interpretation: &str) -> String {
    let handles = super::source_proposals_tests::source_handles(body)
        .expect("host issued source handles in the normal request");
    let handle = &handles["handles"][0];
    let args = json!({"type":"sourceProposal","records":[{
        "sourceId":handle[0],"digest":handle[1],"sourceRevision":handle[2],"partIndex":handle[3],
        "spans":[{"startByte":0,"endByte":handle[4],"role":"body"}],
        "category":category,"interpretation":interpretation
    }]});
    responses::sse(vec![
        responses::ev_function_call("proposal", "blackboard_record_batch", &args.to_string()),
        responses::ev_completed("propose"),
    ])
}

/// Every request the mock model received, answered by `script(sequence, body)`.
async fn mount_model(
    server: &wiremock::MockServer,
    script: fn(usize, &Value) -> String,
) -> Arc<Mutex<Vec<Value>>> {
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

async fn turn(server: &mut TestAppServer, thread: &str, text: &str) -> Result<()> {
    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.to_string(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    Ok(())
}

/// The next turn-driven receipt (Apply and Undo receipts answer their own requests).
async fn receipt(
    server: &mut TestAppServer,
) -> Result<codex_app_server_protocol::StatefulMemoryReceipt> {
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

async fn apply(
    server: &mut TestAppServer,
    project: &str,
    thread: &str,
    entry_id: &str,
    category: StatefulMemoryApplyCategory,
    action: &str,
) -> Result<codex_app_server_protocol::StatefulMemoryReceipt> {
    let response: StatefulMemoryApplyResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryApply {
            request_id,
            params: StatefulMemoryApplyParams {
                thread_id: thread.to_string(),
                expected_project_id: project.to_string(),
                entry_id: entry_id.to_string(),
                expected_revision: 1,
                category,
                client_action_id: action.to_string(),
            },
        })
        .await?;
    Ok(response.receipt)
}

async fn restart(mut server: TestAppServer, home: &TempDir) -> Result<TestAppServer> {
    assert!(server.shutdown_gracefully().await?.success());
    drop(server);
    TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await
}

async fn root_rules(home: &TempDir, project: &str) -> Result<Vec<String>> {
    let store = BlackboardStore::open(&SqliteConfig::new_for_testing(home.path().abs())).await?;
    Ok(store
        .root_projection(RootBlackboardQuery {
            project_id: project.to_string(),
            max_entries: 256,
        })
        .await?
        .data
        .into_iter()
        .filter(|hit| {
            hit.entry.value.kind == codex_project_intelligence::BlackboardKind::Instruction
        })
        .map(|hit| hit.entry.value.content)
        .collect())
}

fn journey_script(sequence: usize, body: &Value) -> String {
    match sequence {
        1 => proposal(
            body,
            "decision",
            "Use SQLite, not Postgres; offline laptop use.",
        ),
        3 => proposal(
            body,
            "ruledOut",
            "The cache is not the cause of the timing.",
        ),
        _ => message("Understood.\nNext: continue the task."),
    }
}

#[tokio::test]
async fn c456_public_capture_to_use_journey_survives_a_cold_new_session_and_forget() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let calls = mount_model(&responses_server, journey_script).await;

    // a. Two standing rules in ordinary declaration form: saved word for word, with a receipt
    //    and its Undo handle, and applied from the very request of the same turn.
    turn(&mut server, &thread, &declaration()).await?;
    let rules = receipt(&mut server).await?;
    assert_eq!(
        (
            rules.kind,
            rules.undoable,
            rules
                .members
                .iter()
                .map(|member| (member.status, member.category, member.text.as_str()))
                .collect::<Vec<_>>()
        ),
        (
            StatefulMemoryReceiptKind::Rules,
            true,
            vec![
                (
                    StatefulMemoryReceiptStatus::Saved,
                    StatefulMemoryReceiptCategory::Rule,
                    NEXT_RULE
                ),
                (
                    StatefulMemoryReceiptStatus::Saved,
                    StatefulMemoryReceiptCategory::Rule,
                    VENDOR_RULE
                ),
            ]
        )
    );
    let first = calls.lock().unwrap()[0].to_string();
    for expected in [
        format!("content={NEXT_RULE}"),
        format!("content={VENDOR_RULE}"),
        " by=user".to_string(),
    ] {
        assert!(first.contains(&expected), "missing {expected}");
    }

    // b. A conversation-only decision: kept whole for review, never applied until the user
    //    applies it, then the user's own words with their reason.
    turn(&mut server, &thread, DECISION).await?;
    let kept = receipt(&mut server).await?;
    assert_eq!(
        (kept.kind, kept.members[0].status, kept.members[0].category),
        (
            StatefulMemoryReceiptKind::Proposals,
            StatefulMemoryReceiptStatus::Proposed,
            StatefulMemoryReceiptCategory::Decision
        )
    );
    assert_eq!(root_rules(&home, &project).await?.len(), 2);
    let decision_id = kept.members[0].entry_id.clone().unwrap();
    let applied = apply(
        &mut server,
        &project,
        &thread,
        &decision_id,
        StatefulMemoryApplyCategory::Decision,
        "apply-decision",
    )
    .await?;
    assert_eq!(
        (applied.kind, applied.members[0].text.as_str()),
        (StatefulMemoryReceiptKind::Promotion, DECISION)
    );

    // c. A rejected hypothesis with its whole reason, applied as the user's ruled-out approach.
    turn(&mut server, &thread, RULED_OUT).await?;
    let kept = receipt(&mut server).await?;
    assert_eq!(
        kept.members[0].category,
        StatefulMemoryReceiptCategory::RuledOut
    );
    let ruled_out = apply(
        &mut server,
        &project,
        &thread,
        kept.members[0].entry_id.as_deref().unwrap(),
        StatefulMemoryApplyCategory::RuledOut,
        "apply-ruled-out",
    )
    .await?;
    assert_eq!(ruled_out.members[0].text, RULED_OUT);
    assert_eq!(
        calls.lock().unwrap().len(),
        5,
        "no extraction or memory-only turn"
    );

    // d. Cold start: a new process and a new thread of the same project, without reminders.
    let mut server = restart(server, &home).await?;
    let second = start_thread(&mut server, &project).await?;
    turn(
        &mut server,
        &second,
        "Which database did we choose and why, and what have we ruled out?",
    )
    .await?;
    let cold = calls.lock().unwrap()[5].to_string();
    for expected in [
        format!("content={NEXT_RULE}"),
        format!("content={VENDOR_RULE}"),
        format!("content={DECISION}"),
        format!("content={RULED_OUT}"),
        "rejectedApproach".to_string(),
        " said=".to_string(),
        " by=user (applied)".to_string(),
    ] {
        assert!(cold.contains(&expected), "cold session lacks {expected}");
    }

    // e. Forget the vendor rule: it stops applying, in later sessions too, and a later
    //    identical statement does not resurrect it.
    let listed: StatefulMemoryReadResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryRead {
            request_id,
            params: StatefulMemoryReadParams {
                thread_id: second.clone(),
                expected_project_id: project.clone(),
                cursor: None,
                limit: None,
                background_section: true,
            },
        })
        .await?;
    let vendor = listed
        .data
        .iter()
        .find(|item| item.content == VENDOR_RULE)
        .unwrap()
        .clone();
    let _: StatefulMemoryForgetResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryForget {
            request_id,
            params: StatefulMemoryForgetParams {
                thread_id: second.clone(),
                expected_project_id: project.clone(),
                entry_id: vendor.entry_id.clone(),
                expected_revision: vendor.revision,
            },
        })
        .await?;
    let mut server = restart(server, &home).await?;
    let third = start_thread(&mut server, &project).await?;
    turn(&mut server, &third, "Continue the work.").await?;
    let after = calls.lock().unwrap()[6].to_string();
    assert!(after.contains(&format!("content={NEXT_RULE}")));
    assert!(!after.contains("vendor/"), "forgotten rule was applied");
    turn(&mut server, &third, &declaration()).await?;
    let restated = receipt(&mut server).await?;
    assert_eq!(
        (
            restated.undoable,
            restated
                .members
                .iter()
                .map(|member| (member.status, member.entry_id.clone()))
                .collect::<Vec<_>>()
        ),
        (
            false,
            vec![
                (StatefulMemoryReceiptStatus::Refused, None),
                (StatefulMemoryReceiptStatus::NotRestored, None),
            ]
        )
    );
    assert_eq!(
        root_rules(&home, &project).await?,
        vec![NEXT_RULE.to_string()]
    );
    Ok(())
}

fn quiet_script(_sequence: usize, _body: &Value) -> String {
    message("Done.")
}

#[tokio::test]
async fn c456_public_quoted_rules_admit_nothing_and_receipt_undo_is_exact() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let _calls = mount_model(&responses_server, quiet_script).await;
    // A fenced copy and a reported list are sealed history, never the user's rules.
    for text in [
        format!("```\n{}\n```", declaration()),
        format!("Lena wrote:\n- {NEXT_RULE}\n- {VENDOR_RULE}"),
        "Always test first.".to_string(),
    ] {
        turn(&mut server, &thread, &text).await?;
    }
    assert_eq!(root_rules(&home, &project).await?, Vec::<String>::new());
    // An explicitly added rule is acknowledged, never duplicated, by a later declaration.
    let _: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Rule,
                content: NEXT_RULE.to_string(),
                scope: None,
                reason: None,
                client_action_id: "explicit-next".to_string(),
                background_section: true,
            },
        })
        .await?;
    turn(&mut server, &thread, &declaration()).await?;
    let saved = receipt(&mut server).await?;
    assert_eq!(
        saved
            .members
            .iter()
            .map(|member| member.status)
            .collect::<Vec<_>>(),
        vec![
            StatefulMemoryReceiptStatus::AlreadyPresent,
            StatefulMemoryReceiptStatus::Saved
        ]
    );
    let undo = |action: &'static str| {
        let (thread, project, receipt_id) =
            (thread.clone(), project.clone(), saved.receipt_id.clone());
        move |request_id| ClientRequest::StatefulMemoryUndo {
            request_id,
            params: StatefulMemoryUndoParams {
                thread_id: thread,
                expected_project_id: project,
                receipt_id,
                client_action_id: action.to_string(),
            },
        }
    };
    let undone: StatefulMemoryUndoResponse = server.request(undo("undo-1")).await?;
    assert_eq!(
        undone
            .receipt
            .members
            .iter()
            .map(|member| member.status)
            .collect::<Vec<_>>(),
        vec![
            StatefulMemoryReceiptStatus::Untouched,
            StatefulMemoryReceiptStatus::Undone
        ]
    );
    // Only the receipt's new rule stopped applying; the earlier explicit rule stays.
    assert_eq!(
        root_rules(&home, &project).await?,
        vec![NEXT_RULE.to_string()]
    );
    let mut server = restart(server, &home).await?;
    let retried: StatefulMemoryUndoResponse = server.request(undo("undo-1")).await?;
    assert_eq!(retried.receipt, undone.receipt);
    assert_eq!(
        root_rules(&home, &project).await?,
        vec![NEXT_RULE.to_string()]
    );
    Ok(())
}
