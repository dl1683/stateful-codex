use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::ContextMapRefreshParams;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulRunBudget;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulRunReadParams;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulRunReadResponse;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulRunStartParams;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulRunStartResponse;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulRunStatus as ApiRunStatus;
#[cfg(not(target_os = "windows"))]
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::BlackboardStore;
#[cfg(not(target_os = "windows"))]
use codex_project_intelligence::RootBlackboardQuery;
#[cfg(not(target_os = "windows"))]
use codex_state::SqliteConfig;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::NewStatefulRun;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::RunBudget;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::StatefulRunId;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::StatefulRunStatus;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::StatefulRunStore;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::StatefulRunUpdate;
#[cfg(not(target_os = "windows"))]
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::AbsolutePathBuf;
#[cfg(not(target_os = "windows"))]
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

#[cfg(not(target_os = "windows"))]
const ORIENTATION_PROMPT: &str = "ORIENTATION_TRANSCRIPT_MARKER Orient yourself in this project.";
#[cfg(not(target_os = "windows"))]
const ORIENTATION_RESULT: &str = "ORIENTATION_RESULT_MARKER The orientation is complete.";
#[cfg(not(target_os = "windows"))]
const CAPTURED_FINDING: &str = "Textkit normalizes text: textkit/cli.py parses arguments and delegates to textkit/core.py; the README documents `python -m pytest` as the test command (not executed).";

/// An orientation that reads sources without changing them captures its reusable
/// findings and completes with durableLearning; a fresh thread receives those findings and
/// the orientation request itself, while run results without conversation are not recalled.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn orientation_findings_and_conversation_reach_a_fresh_thread() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::write_acceptance_files(project_root.path())?;
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
    super::stateful_acceptance_support::seed_admitted_plan(
        codex_home.path(),
        &started.run.id,
        &["README.md"],
    )
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
            super::stateful_acceptance_support::run_check("acceptance-check", project_root.path()),
            tool_call(
                "complete-orientation",
                "stateful_run_update",
                json!({
                    "expectedRevision": run_revision,
                    "status": "completed",
                    "openIssues": [],
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
        complete_seeded_run(
            &store,
            &run_id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Completed,
                strategy: None,
                result: Some(format!("NEWER_OUTCOME_MARKER {index}")),
            },
            None,
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
    // The request refers to earlier work, so the conversation record is part of it.
    run_turn(
        &mut server,
        &second.thread.id,
        "Continue from the orientation: how do I run the tests?",
    )
    .await?;
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

/// A long, quote-heavy thread lists in pages smaller than the default that fit the result
/// budget; following the returned cursors lists every turn exactly once; and an exact read
/// starts from the listing cursor it is given rather than from the newest turn.
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
    let first_thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id.clone()),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    // Quote-heavy requests and answers make 20 previews exceed the result budget.
    let quoted = "\"quoted\" ".repeat(60);
    let _first_log = responses::mount_sse_sequence(
        &responses_server,
        (0..TURNS).map(|_| assistant(&quoted)).collect(),
    )
    .await;
    let request = |index: usize| format!("{index:02} {quoted}");
    for index in 0..TURNS {
        run_turn(&mut server, &first_thread, &request(index)).await?;
    }
    let reader = server
        .start_thread(ThreadStartParams {
            project_id: Some(project_id),
            ..Default::default()
        })
        .await?
        .thread
        .id;

    // Follow the tool's own cursors, one user turn per page.
    let mut listed = Vec::new();
    let mut page_sizes = Vec::new();
    let mut page_cursors = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let log = responses::mount_sse_sequence(
            &responses_server,
            vec![
                tool_call(
                    &format!("list-turns-{}", page_sizes.len()),
                    "conversation_read",
                    json!({"threadId": first_thread, "cursor": cursor}),
                ),
                assistant("Listed."),
            ],
        )
        .await;
        run_turn(&mut server, &reader, "List my earlier turns.").await?;
        let text = log.requests()[1]
            .function_call_output_text(&format!("list-turns-{}", page_sizes.len()))
            .expect("listing output");
        assert!(text.len() <= 9_000, "{} bytes", text.len());
        let page: Value = serde_json::from_str(&text)?;
        let ids = page["turns"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|turn| turn["turnId"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(page_sizes.len() < 12, "listing did not terminate");
        page_sizes.push(ids.len());
        page_cursors.push((cursor.clone(), ids.clone()));
        listed.extend(ids);
        match page["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }
    assert!(
        page_sizes[0] < 20,
        "first page held {} turns",
        page_sizes[0]
    );
    let mut unique = listed.clone();
    unique.sort();
    unique.dedup();
    assert_eq!((listed.len(), unique.len()), (TURNS, TURNS));

    // A turn from a later page reads exactly from that page's cursor, while the newest turn
    // is not found when the read starts from the same older position.
    let (older_cursor, older_ids) = page_cursors
        .iter()
        .find(|(cursor, _)| cursor.is_some())
        .cloned()
        .expect("a second listing page");
    let newest_turn = listed[0].clone();
    let read_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "read-older",
                "conversation_read",
                json!({
                    "threadId": first_thread,
                    "turnId": older_ids[0],
                    "part": "user",
                    "cursor": older_cursor,
                }),
            ),
            tool_call(
                "read-newest-from-older",
                "conversation_read",
                json!({
                    "threadId": first_thread,
                    "turnId": newest_turn,
                    "part": "user",
                    "cursor": older_cursor,
                }),
            ),
            assistant("Read."),
        ],
    )
    .await;
    run_turn(&mut server, &reader, "Read my earlier turn.").await?;
    let requests = read_log.requests();
    let read: Value = serde_json::from_str(
        &requests[1]
            .function_call_output_text("read-older")
            .expect("read output"),
    )?;
    let older_index = TURNS - 1 - page_sizes[0];
    assert_eq!(read["text"], json!(request(older_index)));
    assert!(
        requests[2]
            .function_call_output_text("read-newest-from-older")
            .expect("refusal output")
            .contains("was not found")
    );
    Ok(())
}

/// The first write to a never-indexed project indexes it on the spot instead of failing
/// and sending the model through a refresh round trip.
#[tokio::test]
async fn first_record_in_an_unindexed_project_succeeds() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(project_root.path().join("README.md"), "# Recipes\n")?;
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
                name: "Unindexed".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: "unindexed-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call(
                "first-record",
                "blackboard_record_batch",
                json!({"records": [{
                    "idempotencyKey": "rule-metric-only",
                    "kind": "fact",
                    "content": "The recipe measurements are metric.",
                    "confidenceBasisPoints": 10000,
                    "verification": "unverified",
                    "importance": "high",
                    "rootPromotion": "promoted"
                }]}),
            ),
            assistant("Saved."),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Remember: metric only.").await?;

    let output: Value = serde_json::from_str(
        &log.requests()[1]
            .function_call_output_text("first-record")
            .expect("record output"),
    )?;
    assert_eq!(
        (output["recorded"].clone(), output["failed"].clone()),
        (json!(1), json!(0))
    );
    Ok(())
}

/// A source query in a never-indexed project indexes it on demand and returns routes, so
/// the model never has to discover and call the refresh tool first.
#[tokio::test]
async fn first_source_query_indexes_an_unindexed_project() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("README.md"),
        "# Recipes\n\nScale recipes with recipes.py.\n",
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
                name: "Unindexed query".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: "unindexed-query-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id),
            ..Default::default()
        })
        .await?
        .thread
        .id;
    let log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            tool_call("invalid-query", "context_map_query", json!({"text": "  "})),
            tool_call(
                "first-query",
                "context_map_query",
                json!({"text": "Scale recipes"}),
            ),
            tool_call(
                "second-query",
                "context_map_query",
                json!({"text": "Scale recipes"}),
            ),
            assistant("Found it."),
        ],
    )
    .await;
    run_turn(&mut server, &thread, "Where is scaling documented?").await?;

    let requests = log.requests();
    // An invalid query is refused before any indexing, so the first valid query indexes.
    assert!(
        !requests[1]
            .function_call_output_text("invalid-query")
            .expect("invalid query output")
            .contains("indexedOnDemand")
    );
    let output = |index: usize, call_id: &str| -> Result<Value> {
        Ok(serde_json::from_str(
            &requests[index]
                .function_call_output_text(call_id)
                .expect("query output"),
        )?)
    };
    let first = output(2, "first-query")?;
    assert_eq!(first["indexedOnDemand"], json!(true));
    assert!(
        first["data"]
            .as_array()
            .is_some_and(|routes| !routes.is_empty()),
        "{first}"
    );
    assert_eq!(output(3, "second-query")?["indexedOnDemand"], json!(false));
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

#[cfg(not(target_os = "windows"))]
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

#[cfg(not(target_os = "windows"))]
/// Seeds a completed run through the host completion decision, as the product does.
async fn complete_seeded_run(
    store: &codex_stateful_runtime::StatefulRunStore,
    run_id: &codex_stateful_runtime::StatefulRunId,
    update: StatefulRunUpdate,
    obligation: Option<(String, codex_stateful_runtime::NewObligation)>,
) -> Result<()> {
    let latest = store.latest_obligation(run_id).await?;
    let commit = super::stateful_acceptance_support::seeded_commit(
        store,
        run_id,
        "seed",
        latest.map(|obligation| obligation.sequence),
    )
    .await?;
    store
        .complete_run_with_acceptance(run_id, update, &commit, obligation)
        .await?;
    Ok(())
}
