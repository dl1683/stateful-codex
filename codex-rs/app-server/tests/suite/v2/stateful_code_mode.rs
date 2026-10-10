use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ObligationListParams;
use codex_app_server_protocol::ObligationListResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

/// Prose-bearing Stateful writes are direct model tools even in code-mode-only
/// sessions, so semantic text containing quotes never has to survive a
/// model-written JavaScript string literal.
#[tokio::test]
async fn code_mode_only_keeps_prose_writes_direct_and_quote_safe() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .enable_feature(Feature::CodeModeOnly)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Code mode prose writes".to_string(),
                roots: Vec::new(),
                metadata: None,
                idempotency_key: "code-mode-prose-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: "Record a quoted finding.".to_string(),
                mode: StatefulWorkflowMode::Collaborative,
                budget: StatefulRunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "code-mode-prose-run".to_string(),
            },
        })
        .await?;
    let quoted_learning = r#"The "Top 20 Customers" tab names DOE as the largest customer."#;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "quoted-obligation",
                    "obligation_update",
                    &json!({
                        "idempotencyKey": "quoted-obligation",
                        "packet": {
                            "learning": [quoted_learning],
                            "next": [r#"Check the "DOE IDIQ" tab before valuation."#]
                        }
                    })
                    .to_string(),
                ),
                responses::ev_completed("quoted-obligation-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("recorded-message", "Recorded."),
                responses::ev_completed("recorded-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Record what the customer tab shows.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    let tools = requests[0].body_json()["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let tool_name = |tool: &Value| tool["name"].as_str().unwrap_or_default().to_string();
    let top_level = tools.iter().map(tool_name).collect::<Vec<_>>();
    // This test model has no tool search, so the deferred Stateful tools fall back to
    // direct exposure instead of becoming unreachable.
    for direct in [
        "obligation_update",
        "stateful_run_update",
        "stateful_run_read",
        "blackboard_record_batch",
        "blackboard_relate",
        "blackboard_update_batch",
        "steering_reconcile",
        "conversation_read",
    ] {
        assert!(
            top_level.iter().any(|name| name == direct),
            "{direct} should be a direct model tool; saw {top_level:?}"
        );
    }
    let nested = tools
        .iter()
        .find(|tool| tool_name(tool) == "exec")
        .and_then(|tool| tool["description"].as_str())
        .unwrap_or_default()
        .to_string();
    // Memory-bearing Stateful outputs are delivered only to direct calls, where the host
    // checks them against Forget, Undo and correction as they are recorded.
    assert!(!nested.contains("blackboard_query("));
    assert!(!nested.contains("evidence_read("));
    assert!(!nested.contains("obligation_update("));
    assert!(!nested.contains("blackboard_record("));

    let obligations: ObligationListResponse = server
        .request(|request_id| ClientRequest::ObligationList {
            request_id,
            params: ObligationListParams {
                run_id: started.run.id,
                cursor: None,
                limit: Some(10),
            },
        })
        .await?;
    assert_eq!(
        obligations
            .data
            .iter()
            .map(|obligation| obligation.packet.learning.clone())
            .collect::<Vec<_>>(),
        vec![vec![quoted_learning.to_string()]]
    );
    Ok(())
}

/// Nested Code Mode cells cannot call Stateful tools: a cell can hold a result before printing
/// it, outside the recording where the host checks memory outputs. The tools stay direct.
#[tokio::test]
async fn code_mode_cells_cannot_reach_stateful_memory_tools() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    let source = "\"\"\"Amount parsing used by the importers.\"\"\"\n\n\ndef parse_amount(text):\n    return int(text)\n";
    std::fs::write(project_root.path().join("amounts.py"), source)?;
    MockResponsesConfig::new(&responses_server.uri())
        .enable_feature(Feature::Sqlite)
        .enable_feature(Feature::CodeModeOnly)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Code mode evidence".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root should be absolute"),
                }],
                metadata: None,
                idempotency_key: "code-mode-evidence-project".to_string(),
            },
        })
        .await?;
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: project.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id),
            ..Default::default()
        })
        .await?;
    let response_log = responses::mount_sse_sequence(
        &responses_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("evidence-response"),
                responses::ev_custom_tool_call(
                    "evidence-exec",
                    "exec",
                    r#"
text(JSON.stringify([typeof tools.evidence_read, typeof tools.blackboard_query, typeof tools.memory_read]));
"#,
                ),
                responses::ev_completed("evidence-response"),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("done-message", "Done"),
                responses::ev_completed("done-response"),
            ]),
        ],
    )
    .await;

    server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.thread.id,
            input: vec![UserInput::Text {
                text: "Where is parse_amount defined?".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    let output = requests[1].custom_tool_call_output("evidence-exec");
    let printed = output["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .find_map(|text| serde_json::from_str::<Value>(text).ok())
        .unwrap_or_else(|| panic!("the script should print the evidence result: {output}"));
    assert_eq!(printed, json!(["undefined", "undefined", "undefined"]));
    let direct = requests[0].body_json()["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert!(
        direct.iter().any(|name| name == "evidence_read"),
        "evidence_read stays a direct model tool; saw {direct:?}"
    );
    Ok(())
}
