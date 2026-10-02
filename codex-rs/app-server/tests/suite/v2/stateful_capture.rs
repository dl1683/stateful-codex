use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus as ApiRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

const ORIENTATION_PROMPT: &str = "ORIENTATION_TRANSCRIPT_MARKER Orient yourself in this project.";
const ORIENTATION_RESULT: &str = "ORIENTATION_RESULT_MARKER The orientation is complete.";
const CAPTURED_FINDING: &str = "Textkit normalizes text: textkit/cli.py parses arguments and delegates to textkit/core.py; the README documents `python -m pytest` as the test command (not executed).";

/// An orientation that reads sources without changing them captures its reusable
/// findings and completes with durableLearning; a fresh thread receives those findings and
/// the orientation request itself, while run results without conversation are not recalled.
#[tokio::test]
async fn orientation_findings_and_conversation_reach_a_fresh_thread() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("README.md"),
        "# Textkit\n\nTextkit normalizes text. `textkit/cli.py` parses arguments and delegates to `textkit/core.py`.\n\nRun the tests with `python -m pytest`.\n",
    )?;
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
                name: "Capture orientation".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: "capture-orientation-project".to_string(),
            },
        })
        .await?;
    let project_id = project.project.id;
    server
        .request::<ContextMapRefreshResponse>(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: project_id.clone(),
            },
        })
        .await?;
    let first = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?;
    let first_thread_id = first.thread.id;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project_id.clone(),
                thread_id: first_thread_id.clone(),
                goal: "Orient yourself in this project.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "capture-orientation-run".to_string(),
            },
        })
        .await?;

    // Read the source; the host issues a read receipt.
    let read_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "read-readme",
                "evidence_read",
                json!({"relativePath": "README.md"}),
            ),
            assistant("README reviewed."),
        ],
    )
    .await;
    run_turn(&mut server, &first_thread_id, ORIENTATION_PROMPT).await?;
    let read_output: Value = serde_json::from_str(
        &read_log
            .function_call_output_text("read-readme")
            .expect("evidence output should be text"),
    )?;
    let receipt_id = read_output["blackboardEvidence"]["readReceiptId"]
        .as_str()
        .expect("a complete read carries a receipt")
        .to_string();

    // Capture the reusable finding against that receipt and promote it.
    let record_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "record-orientation",
                "blackboard_record_batch",
                json!({"records": [{
                    "idempotencyKey": "orientation-module-map",
                    "kind": "fact",
                    "content": CAPTURED_FINDING,
                    "confidenceBasisPoints": 9000,
                    "verification": "sourceVerified",
                    "importance": "high",
                    "rootPromotion": "promoted",
                    "evidence": [{"readReceiptId": receipt_id}]
                }]}),
            ),
            assistant("Findings recorded."),
        ],
    )
    .await;
    run_turn(
        &mut server,
        &first_thread_id,
        "Record what is worth reusing.",
    )
    .await?;
    let recorded: Value = serde_json::from_str(
        &record_log
            .function_call_output_text("record-orientation")
            .expect("batch output should be text"),
    )?;
    assert_eq!(
        (&recorded["recorded"], &recorded["failed"]),
        (&json!(1), &json!(0))
    );

    // Complete with durableLearning, selecting the promoted finding.
    let sqlite = SqliteConfig::new_for_testing(codex_home.path().abs());
    let root_revision = BlackboardStore::open(&sqlite)
        .await?
        .root_projection(RootBlackboardQuery {
            project_id: project_id.clone(),
            max_entries: 256,
        })
        .await?
        .revision;
    let run_revision = read_run(&mut server, &started.run.id).await?.revision;
    let complete_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "complete-orientation",
                "stateful_run_update",
                json!({
                    "expectedRevision": run_revision,
                    "status": "completed",
                    "result": ORIENTATION_RESULT,
                    "rootRevision": root_revision,
                    "materialRootFindings": ["E1"],
                    "completionIdempotencyKey": "orientation-final",
                    "finalObligation": {"learning": ["ORIENTATION_LEARNING_MARKER module map captured."]}
                }),
            ),
            assistant("Orientation complete."),
        ],
    )
    .await;
    run_turn(&mut server, &first_thread_id, "Finish the orientation.").await?;
    let completion_text = complete_log
        .function_call_output_text("complete-orientation")
        .expect("completion output should be text");
    let completion: Value = serde_json::from_str(&completion_text)
        .unwrap_or_else(|_| panic!("completion should succeed: {completion_text}"));
    let stored = read_run(&mut server, &started.run.id).await?;
    let stored_result = stored.result.unwrap_or_default();
    // The durable basis carries the selected finding and the final learning.
    assert_eq!(
        (
            &completion["status"],
            stored.status,
            stored_result.starts_with(ORIENTATION_RESULT),
            stored_result.contains(CAPTURED_FINDING),
            stored_result.contains("ORIENTATION_LEARNING_MARKER"),
        ),
        (
            &json!("completed"),
            ApiRunStatus::Completed,
            true,
            true,
            true
        )
    );

    // Five newer completed outcomes fill the recent-outcome window.
    let store = StatefulRunStore::open(&sqlite).await?;
    for index in 0..5 {
        let run_id = StatefulRunId::parse(format!("zz-newer-outcome-{index}"))?;
        let run = store
            .create_run(
                run_id.clone(),
                NewStatefulRun {
                    project_id: project_id.clone(),
                    thread_ids: vec![first_thread_id.clone()],
                    goal: format!("Routine task {index}."),
                    mode: WorkflowMode::Collaborative,
                    budget: RunBudget {
                        max_continuations: 1,
                        max_elapsed_seconds: 3_600,
                    },
                },
            )
            .await?;
        store
            .update_run(
                &run_id,
                StatefulRunUpdate {
                    expected_revision: run.revision,
                    status: StatefulRunStatus::Completed,
                    strategy: None,
                    result: Some(format!("NEWER_OUTCOME_MARKER {index}")),
                },
            )
            .await?;
    }

    let second = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id),
            ..Default::default()
        })
        .await?;
    let reuse_log =
        responses::mount_sse_sequence(&responses_server, vec![assistant("Reused.")]).await;
    run_turn(&mut server, &second.thread.id, "How do I run the tests?").await?;
    let body = reuse_log.single_request().body_json().to_string();
    let observed = [
        "NEWER_OUTCOME_MARKER 0",
        "NEWER_OUTCOME_MARKER 4",
        CAPTURED_FINDING,
        "ORIENTATION_RESULT_MARKER",
        "ORIENTATION_LEARNING_MARKER",
        "ORIENTATION_TRANSCRIPT_MARKER",
    ]
    .map(|marker| (marker, body.contains(marker)));
    assert_eq!(
        observed,
        [
            // Run results with no conversation behind them are not recalled.
            ("NEWER_OUTCOME_MARKER 0", false),
            ("NEWER_OUTCOME_MARKER 4", false),
            (CAPTURED_FINDING, true),
            ("ORIENTATION_RESULT_MARKER", false),
            ("ORIENTATION_LEARNING_MARKER", false),
            // The orientation request is recalled from the earlier thread's turn summary.
            ("ORIENTATION_TRANSCRIPT_MARKER", true),
        ]
    );
    Ok(())
}

/// A fresh thread sees an earlier request shortened in its continuity record and browses
/// to it with conversation_read: the project's threads, that thread's turns, and the exact
/// text, paged at a character boundary within the result budget.
#[tokio::test]
async fn model_reads_a_shortened_earlier_turn_in_full() -> Result<()> {
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
                metadata: None,
                idempotency_key: "recall-project".to_string(),
            },
        })
        .await?;
    let project_id = project.project.id;
    let first = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?;
    // Multi-byte and quote-heavy text exercises character boundaries and escaping.
    let long_request = format!("{}TAIL_OF_THE_LONG_REQUEST", "\u{20ac}\"<".repeat(2_000));
    let _first_log =
        responses::mount_sse_sequence(&responses_server, vec![assistant("Planned.")]).await;
    let completed = server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: first.thread.id.clone(),
            input: vec![UserInput::Text {
                text: long_request.clone(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn_id = completed.turn.id;
    let first_thread = first.thread.id;

    let second = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id),
            ..Default::default()
        })
        .await?;
    let recall_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call("list-threads", "conversation_read", json!({})),
            tool_call(
                "list-turns",
                "conversation_read",
                json!({"threadId": first_thread}),
            ),
            tool_call(
                "read-turn",
                "conversation_read",
                json!({"threadId": first_thread, "turnId": turn_id, "part": "user"}),
            ),
            assistant("Recalled."),
        ],
    )
    .await;
    run_turn(&mut server, &second.thread.id, "What did I ask before?").await?;

    let requests = recall_log.requests();
    assert_eq!(requests.len(), 4);
    let first_body = requests[0].body_json().to_string();
    assert!(first_body.contains(&format!(
        "of {} bytes; conversation_read threadId=",
        long_request.len()
    )));
    assert!(!first_body.contains("TAIL_OF_THE_LONG_REQUEST"));
    let output = |index: usize, call_id: &str| -> Result<Value> {
        Ok(serde_json::from_str(
            &requests[index]
                .function_call_output_text(call_id)
                .expect("tool output"),
        )?)
    };
    let threads = output(1, "list-threads")?;
    assert!(
        threads["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|thread| thread["threadId"] == json!(first_thread))
    );
    let turns = output(2, "list-turns")?;
    assert_eq!(turns["turns"][0]["turnId"], json!(turn_id));
    assert_eq!(turns["turns"][0]["answer"], json!("Planned."));
    assert_eq!(turns["turns"][0]["userBytes"], json!(long_request.len()));
    let read_text = requests[3]
        .function_call_output_text("read-turn")
        .expect("read output");
    assert!(read_text.len() <= 9_000);
    let read: Value = serde_json::from_str(&read_text)?;
    let page = read["text"].as_str().expect("page text");
    let next_offset = read["nextOffset"]
        .as_u64()
        .and_then(|offset| usize::try_from(offset).ok())
        .expect("more text remains");
    assert_eq!(next_offset, page.len());
    assert!(long_request.is_char_boundary(next_offset));
    assert_eq!(page, &long_request[..next_offset]);
    Ok(())
}

/// A long, quote-heavy thread lists in pages that fit the result budget and keep a cursor,
/// and a turn beyond the first page is read exactly by starting from its listing cursor.
#[tokio::test]
async fn conversation_read_pages_long_threads_within_the_result_budget() -> Result<()> {
    const TURNS: usize = 25;
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
                name: "Paging".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "paging-project".to_string(),
            },
        })
        .await?;
    let project_id = project.project.id;
    let first = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?;
    let first_thread = first.thread.id;
    let _first_log = responses::mount_sse_sequence(
        &responses_server,
        (0..TURNS).map(|_| assistant("\"Noted.\"")).collect(),
    )
    .await;
    let request = |index: usize| format!("{index:02} {}", "\"quoted\" ".repeat(60));
    for index in 0..TURNS {
        run_turn(&mut server, &first_thread, &request(index)).await?;
    }
    // The second ten turns, newest first, through the public turn listing.
    let first_page = turn_page(&mut server, &first_thread, /*cursor*/ None).await?;
    let older_cursor = first_page.next_cursor.expect("older turns remain");
    let second_page = turn_page(&mut server, &first_thread, Some(older_cursor.clone())).await?;
    let older_turn = second_page.data[0].id.clone();

    let second = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id),
            ..Default::default()
        })
        .await?;
    let paging_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "list-turns",
                "conversation_read",
                json!({"threadId": first_thread}),
            ),
            tool_call(
                "read-older",
                "conversation_read",
                json!({
                    "threadId": first_thread,
                    "turnId": older_turn,
                    "part": "user",
                    "cursor": older_cursor,
                }),
            ),
            assistant("Read."),
        ],
    )
    .await;
    run_turn(&mut server, &second.thread.id, "List my earlier turns.").await?;

    let requests = paging_log.requests();
    assert_eq!(requests.len(), 3);
    let listing_text = requests[1]
        .function_call_output_text("list-turns")
        .expect("listing output");
    assert!(listing_text.len() <= 9_000, "{} bytes", listing_text.len());
    let listing: Value = serde_json::from_str(&listing_text)?;
    let listed = listing["turns"].as_array().map_or(0, Vec::len);
    assert!(listed > 0 && listed < TURNS, "{listed} turns listed");
    assert!(listing["nextCursor"].is_string());
    let read: Value = serde_json::from_str(
        &requests[2]
            .function_call_output_text("read-older")
            .expect("read output"),
    )?;
    assert_eq!(read["turnId"], json!(older_turn));
    assert_eq!(read["text"], json!(request(TURNS - 11)));
    Ok(())
}

async fn turn_page(
    server: &mut TestAppServer,
    thread_id: &str,
    cursor: Option<String>,
) -> Result<ThreadTurnsListResponse> {
    server
        .request(|request_id| ClientRequest::ThreadTurnsList {
            request_id,
            params: ThreadTurnsListParams {
                thread_id: thread_id.to_string(),
                cursor,
                limit: Some(10),
                sort_direction: None,
                items_view: None,
            },
        })
        .await
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

async fn read_run(
    server: &mut TestAppServer,
    run_id: &str,
) -> Result<codex_app_server_protocol::StatefulRun> {
    Ok(server
        .request::<StatefulRunReadResponse>(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(run_id.to_string()),
                thread_id: None,
            },
        })
        .await?
        .run
        .expect("run should be readable"))
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
