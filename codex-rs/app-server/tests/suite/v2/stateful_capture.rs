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
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::RootBlackboardQuery;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::ObligationPacket;
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
/// findings, completes with durableLearning, and a fresh thread still receives those
/// findings after five newer outcomes have pushed the orientation result out of the
/// recent-outcome window.
#[tokio::test]
async fn orientation_findings_reach_a_fresh_thread_after_outcome_eviction() -> Result<()> {
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
    let run_revision = server
        .request::<StatefulRunReadResponse>(|request_id| ClientRequest::StatefulRunRead {
            request_id,
            params: StatefulRunReadParams {
                run_id: Some(started.run.id.clone()),
                thread_id: None,
            },
        })
        .await?
        .run
        .expect("orientation run should be readable")
        .revision;
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
    assert_eq!(completion["status"], json!("completed"));

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
            .complete_run_with_obligation(
                &run_id,
                StatefulRunUpdate {
                    expected_revision: run.revision,
                    status: StatefulRunStatus::Completed,
                    strategy: None,
                    result: Some(format!("NEWER_OUTCOME_MARKER {index}")),
                },
                format!("newer-outcome-final-{index}"),
                NewObligation {
                    project_id: project_id.clone(),
                    run_id: run_id.clone(),
                    packet: ObligationPacket {
                        implication: vec![format!("Routine task {index} needs no follow-up.")],
                        ..Default::default()
                    },
                    provenance_source_id: "newer-outcome-fixture".to_string(),
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
            ("NEWER_OUTCOME_MARKER 0", true),
            ("NEWER_OUTCOME_MARKER 4", true),
            (CAPTURED_FINDING, true),
            ("ORIENTATION_RESULT_MARKER", false),
            ("ORIENTATION_LEARNING_MARKER", false),
            ("ORIENTATION_TRANSCRIPT_MARKER", false),
        ]
    );
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
