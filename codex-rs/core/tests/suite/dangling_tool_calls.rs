//! A thread whose history ends mid tool call (a fork of a live turn, or a resume after
//! a hard kill) must still sample: the dangling call gets an "aborted" output.

use std::sync::Arc;

use codex_core::CodexThread;
use codex_core::ForkSnapshot;
use codex_core::TurnInputRequest;
use codex_history::InitialHistory;
use codex_history::ResumedHistory;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::mcp::ClientMcpExtensions;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use wiremock::MockServer;

/// A turn cut while a code-mode `exec` call was in flight, after a shell call returned.
fn history_cut_mid_tool_call(thread_id: ThreadId) -> anyhow::Result<InitialHistory> {
    let items = [
        json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Run the long command."}]}),
        json!({"type": "function_call", "name": "shell", "arguments": "{}", "call_id": "shell-returned"}),
        json!({"type": "function_call_output", "call_id": "shell-returned", "output": "ok"}),
        json!({"type": "custom_tool_call", "call_id": "exec-dangling", "name": "exec", "input": "sleep 420"}),
    ]
    .into_iter()
    .map(|item| {
        Ok(RolloutItem::ResponseItem(
            serde_json::from_value::<ResponseItem>(item)?.into(),
        ))
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(InitialHistory::Resumed(ResumedHistory {
        history_revision: None,
        conversation_id: thread_id,
        history: Arc::new(items),
        rollout_path: None,
    }))
}

async fn mount_reply(server: &MockServer) -> ResponseMock {
    mount_sse_once(
        server,
        sse(vec![
            ev_response_created("resp-continue"),
            ev_assistant_message("msg-continue", "Picked up where the work stopped."),
            ev_completed("resp-continue"),
        ]),
    )
    .await
}

async fn continue_thread(thread: &CodexThread) -> anyhow::Result<()> {
    thread
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Pick up the work.".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    wait_for_event(thread, |event| matches!(event, EventMsg::TurnComplete(_))).await;
    Ok(())
}

/// The call items of the request, in order, as (type, call_id, output).
fn call_items(mock: &ResponseMock) -> Vec<(String, String, Option<String>)> {
    mock.single_request()
        .input()
        .into_iter()
        .filter(|item| item.get("call_id").is_some())
        .map(|item| {
            (
                item["type"].as_str().unwrap_or_default().to_string(),
                item["call_id"].as_str().unwrap_or_default().to_string(),
                item.get("output")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )
        })
        .collect()
}

fn expected_call_items() -> Vec<(String, String, Option<String>)> {
    vec![
        (
            "function_call".to_string(),
            "shell-returned".to_string(),
            None,
        ),
        (
            "function_call_output".to_string(),
            "shell-returned".to_string(),
            Some("ok".to_string()),
        ),
        (
            "custom_tool_call".to_string(),
            "exec-dangling".to_string(),
            None,
        ),
        (
            "custom_tool_call_output".to_string(),
            "exec-dangling".to_string(),
            Some("aborted".to_string()),
        ),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fork_of_a_turn_cut_mid_tool_call_samples_with_an_aborted_output() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;
    let test = test_codex().build_with_auto_env(&server).await?;
    let mock = mount_reply(&server).await;

    let fork = test
        .thread_manager
        .fork_thread_from_history(
            ForkSnapshot::Interrupted,
            codex_core::StartThreadOptions::new(test.config.clone()),
            history_cut_mid_tool_call(ThreadId::new())?,
        )
        .await?
        .thread;
    continue_thread(&fork).await?;

    assert_eq!(call_items(&mock), expected_call_items());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_after_a_kill_mid_tool_call_samples_with_an_aborted_output() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;
    let test = test_codex().build_with_auto_env(&server).await?;
    let mock = mount_reply(&server).await;

    // The killed process left a persisted thread whose history ends mid call.
    let thread_id = test.session_configured.thread_id;
    test.codex.ensure_rollout_materialized().await;
    test.codex.shutdown_and_wait().await?;
    test.thread_manager.remove_thread(&thread_id).await;
    let resumed = test
        .thread_manager
        .resume_thread_with_history(
            test.config.clone(),
            history_cut_mid_tool_call(thread_id)?,
            codex_core::test_support::auth_manager_from_auth(codex_login::CodexAuth::from_api_key(
                "dummy",
            )),
            /*parent_trace*/ None,
            ClientMcpExtensions::default(),
        )
        .await?
        .thread;
    continue_thread(&resumed).await?;

    assert_eq!(call_items(&mock), expected_call_items());
    Ok(())
}
