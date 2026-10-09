//! C4-C6 repair 1 boundaries through the public app-server: multipart messages are never
//! admitted; applied proposals stay out of model tool outputs (across a PostToolUse window);
//! Apply settles only a whole cited message with no unresolved scope; receipts mark
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

const DECISION: &str =
    "We'll use SQLite rather than Postgres, because the tool must run offline on a laptop.";

type Script = Box<dyn Fn(usize, &Value) -> String + Send + Sync>;

fn message(text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message("answer", text),
        responses::ev_completed("answer"),
    ])
}

fn call(id: &str, tool: &str, args: Value) -> String {
    responses::sse(vec![
        responses::ev_function_call(id, tool, &args.to_string()),
        responses::ev_completed(id),
    ])
}

/// A same-turn proposal citing `[0, end)` of the newest sealed part (`None`: the whole part).
fn proposal(
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

async fn mount(server: &wiremock::MockServer, script: Script) -> Arc<Mutex<Vec<Value>>> {
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

async fn turn(server: &mut TestAppServer, thread: &str, parts: &[&str]) -> Result<()> {
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

async fn kept(server: &mut TestAppServer) -> Result<StatefulMemoryReceipt> {
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

fn apply_request(
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

async fn root_contents(home: &TempDir, project: &str) -> Result<Vec<String>> {
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
async fn c456r1_long_receipts_are_marked_shortened_and_replay_is_truthful() -> Result<()> {
    let (home, mut server, project, thread, responses_server) =
        super::capture_sources_tests::setup().await?;
    let long_decision = format!(
        "We'll keep the {} ledger format, because the auditors parse it; only for exports.",
        "é".repeat(120)
    );
    let _calls = mount(
        &responses_server,
        Box::new(|sequence, body| match sequence {
            1 => proposal(
                body,
                "decision",
                "Keep the ledger format for auditors.",
                None,
                None,
            ),
            _ => message("Noted."),
        }),
    )
    .await;
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
    turn(&mut server, &thread, &[&long_decision]).await?;
    let receipt = kept(&mut server).await?;
    assert_eq!(receipt.members[0].applies_text_shortened, true);
    let id = receipt.members[0].entry_id.clone().expect("kept entry");
    let applied: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-long",
        ))
        .await?;
    assert_eq!(applied.receipt.members[0].text_shortened, true);
    // The exact whole words, multibyte included, are recoverable at the receipt's ID@REV.
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
    let stored = listed
        .data
        .iter()
        .find(|item| Some(&item.entry_id) == applied.receipt.members[0].entry_id.as_ref())
        .expect("applied entry listed");
    assert_eq!(
        (stored.revision, stored.content.as_str()),
        (2, long_decision.as_str())
    );
    // Restart: the same action replays the same receipt while it is still applied.
    let mut server = restart(server, &home).await?;
    let replayed: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-long",
        ))
        .await?;
    assert_eq!(replayed.receipt, applied.receipt);
    // After Undo the same action's retry refuses; nothing is restored or announced as applied.
    let undo: StatefulMemoryUndoResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryUndo {
            request_id,
            params: StatefulMemoryUndoParams {
                thread_id: thread.clone(),
                expected_project_id: project.clone(),
                receipt_id: applied.receipt.receipt_id.clone(),
                client_action_id: "undo-long".to_string(),
            },
        })
        .await?;
    assert_eq!(undo.receipt.kind, StatefulMemoryReceiptKind::Undo);
    let before =
        super::model_retirement_tests::snapshot(&SqliteConfig::new_for_testing(home.path().abs()))
            .await?;
    let message = refused(
        &mut server,
        apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-long",
        ),
    )
    .await?;
    assert!(message.contains("nothing was restored"), "{message}");
    let mut server = restart(server, &home).await?;
    let message = refused(
        &mut server,
        apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-long",
        ),
    )
    .await?;
    assert!(message.contains("nothing was restored"), "{message}");
    assert_eq!(
        super::model_retirement_tests::snapshot(&SqliteConfig::new_for_testing(home.path().abs()))
            .await?,
        before
    );
    assert!(
        !root_contents(&home, &project)
            .await?
            .contains(&long_decision)
    );
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

/// Apply → the model reads the applied entry and recalls memory → a PostToolUse hook holds
/// the outputs → the user's Forget commits → the hook releases. The outputs the model then
/// receives never contained the applied words: applied proposals stay out of tool outputs.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn c456r1_applied_words_never_reach_tool_outputs_across_a_post_tool_hook() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    let latch = home.path().join("latch");
    std::fs::create_dir_all(&latch)?;
    let hook = home.path().join("latch_hook.py");
    std::fs::write(
        &hook,
        format!(
            "import os, sys, time\nopen(os.path.join({latch:?}, 'started'), 'w').close()\nwhile not os.path.exists(os.path.join({latch:?}, 'release')):\n    time.sleep(0.05)\n",
            latch = latch.display().to_string()
        ),
    )?;
    std::fs::write(
        home.path().join("requirements.toml"),
        format!(
            "[hooks]\n\n[[hooks.PostToolUse]]\nmatcher = '^(blackboard_query|memory_read)$'\n\n[[hooks.PostToolUse.hooks]]\ntype = 'command'\ncommand = 'python3 {}'\n",
            hook.display()
        ),
    )?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Latch".into(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "latch-project".into(),
            },
        })
        .await?;
    let project = project.project.id;
    let thread = start_thread(&mut server, &project).await?;
    let applied_id = Arc::new(Mutex::new(String::new()));
    let reader = applied_id.clone();
    let calls = mount(
        &responses_server,
        Box::new(move |sequence, body| match sequence {
            0 => proposal(body, "decision", "SQLite for offline laptops.", None, None),
            2 => call(
                "exact",
                "blackboard_query",
                json!({"entryId": reader.lock().expect("id lock").clone()}),
            ),
            3 => call(
                "recall",
                "memory_read",
                json!({"question": "SQLite Postgres offline laptop decision"}),
            ),
            _ => message("Done."),
        }),
    )
    .await;
    turn(&mut server, &thread, &[DECISION]).await?;
    let id = kept(&mut server).await?.members[0]
        .entry_id
        .clone()
        .expect("kept entry");
    *applied_id.lock().expect("id lock") = id.clone();
    let applied: StatefulMemoryApplyResponse = server
        .request(apply_request(
            &project,
            &thread,
            &id,
            StatefulMemoryApplyCategory::Decision,
            "apply-latch",
        ))
        .await?;
    let revision = applied.receipt.members[0]
        .revision
        .expect("applied revision");
    let turn_id = server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: "What did we decide about the database?".into(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let _ = server
        .read_stream_until_response_message(codex_app_server_protocol::RequestId::Integer(turn_id))
        .await?;
    let started = latch.join("started");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !started.exists() {
        assert!(std::time::Instant::now() < deadline, "hook never ran");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
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
    std::fs::write(latch.join("release"), "")?;
    server
        .read_stream_until_notification_message("turn/completed")
        .await?;
    let bodies = calls.lock().expect("request log lock").clone();
    for (body, call_id) in [(&bodies[3], "exact"), (&bodies[4], "recall")] {
        let output = body["input"]
            .as_array()
            .expect("input items")
            .iter()
            .find(|item| item["call_id"] == call_id && item["type"] == "function_call_output")
            .expect("tool output delivered")["output"]
            .to_string();
        assert!(
            !output.contains("offline on a laptop") && !output.contains("SQLite rather than"),
            "{call_id} disclosed applied words: {output}"
        );
    }
    Ok(())
}
