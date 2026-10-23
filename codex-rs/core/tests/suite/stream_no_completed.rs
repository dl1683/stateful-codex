//! Verifies that the agent retries when the SSE stream terminates before
//! delivering a `response.completed` event.

use codex_core::TurnInputRequest;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutRecorder;
use core_test_support::TestTargetOs;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::test_target_os;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use std::net::TcpListener;
use wiremock::MockServer;

fn sse_incomplete() -> String {
    responses::sse(vec![serde_json::json!({
        "type": "response.output_item.done",
    })])
}

fn marker_command() -> &'static str {
    match test_target_os() {
        TestTargetOs::Linux | TestTargetOs::MacOs => "printf 'executed\\n' >> retry-marker.txt",
        TestTargetOs::Windows => {
            "Add-Content -LiteralPath retry-marker.txt -Value executed -Encoding Ascii"
        }
    }
}

/// Clears the per-turn metadata the recorder attaches, so retained items compare to fixtures.
fn without_turn_metadata(mut item: ResponseItem) -> ResponseItem {
    if let ResponseItem::Reasoning {
        internal_chat_message_metadata_passthrough,
        ..
    } = &mut item
    {
        *internal_chat_message_metadata_passthrough = None;
    }
    item
}

fn normalize_outbound_items(mut items: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    for item in &mut items {
        // Fixtures omit transport metadata and summary-only reasoning's null content.
        if item["type"] == "reasoning" {
            let object = item.as_object_mut().expect("reasoning item object");
            object.remove("content");
            object.remove("internal_chat_message_metadata_passthrough");
        } else if item["type"] == "function_call" {
            item.as_object_mut()
                .expect("function call item object")
                .remove("internal_chat_message_metadata_passthrough");
        }
    }
    items
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_filters_interrupted_reasoning_without_replaying_tools() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
            config.model_provider.supports_websockets = false;
        })
        .build_with_auto_env(&server)
        .await?;
    // Request preparation preserves prefixed IDs and strips legacy unprefixed IDs.
    let earlier = responses::ev_reasoning_item("rs_earlier", &["before the tool"], &[]);
    let interrupted = vec![
        responses::ev_reasoning_item("rs_interrupted_1", &["unfinished response"], &[]),
        responses::ev_reasoning_item("rs_interrupted_2", &["still unfinished"], &[]),
    ];
    // A sequence is needed because the tool continuation and retry happen within
    // one turn, before the test regains control to mount another one-shot mock.
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                earlier.clone(),
                responses::ev_function_call(
                    "write-marker",
                    "exec_command",
                    &serde_json::json!({"cmd": marker_command()}).to_string(),
                ),
                responses::ev_completed("tool-response"),
            ]),
            // Closing the stream without response.completed exercises the failure
            // after OutputItemDone has already persisted both reasoning items.
            responses::sse(interrupted.clone()),
            responses::sse(vec![
                responses::ev_assistant_message("done", "finished"),
                responses::ev_completed("retry-response"),
            ]),
        ],
    )
    .await;

    test.submit_turn("Write the marker once, then finish")
        .await?;
    let requests = mock.requests();
    assert_eq!(requests.len(), 3);
    let initial_input = requests[1].input();
    let retry_input = requests[2].input();
    assert_eq!(retry_input, initial_input);
    assert_eq!(
        normalize_outbound_items(requests[2].inputs_of_type("reasoning")),
        vec![earlier["item"].clone()],
    );
    assert_eq!(
        retry_input.last().map(|item| &item["type"]),
        Some(&serde_json::json!("function_call_output")),
    );
    assert_eq!(
        requests[1].function_call_output("write-marker"),
        requests[2].function_call_output("write-marker"),
    );
    let contents = test
        .fs()
        .read_file_text(
            &test.workspace_path_uri("retry-marker.txt")?,
            Default::default(),
            /*sandbox*/ None,
        )
        .await?;
    assert_eq!(contents.lines().collect::<Vec<_>>(), vec!["executed"]);

    test.codex.flush_rollout().await?;
    let (rollout, _, _) =
        RolloutRecorder::load_rollout_items(&test.codex.rollout_path().expect("rollout path"))
            .await?;
    let reasoning = rollout
        .into_iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(envelope)
                if matches!(envelope.item, ResponseItem::Reasoning { .. }) =>
            {
                Some(without_turn_metadata(envelope.item))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let expected = std::iter::once(earlier)
        .chain(interrupted)
        .map(|event| serde_json::from_value::<ResponseItem>(event["item"].clone()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(reasoning, expected);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_retains_reasoning_before_interrupted_tool_output() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let test = test_codex()
        .with_config(|config| {
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
            config.model_provider.supports_websockets = false;
        })
        .build_with_auto_env(&server)
        .await?;
    let mut call = responses::ev_function_call(
        "write-marker",
        "exec_command",
        &serde_json::json!({"cmd": marker_command()}).to_string(),
    );
    call["item"]["id"] = serde_json::json!("fc_write_marker");
    let reasoning = responses::ev_reasoning_item("rs_interrupted_1", &["after the call"], &[]);
    // Both events are delivered before EOF. Pending tools are drained only after
    // stream processing, so their output deterministically follows the reasoning.
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![call.clone(), reasoning.clone()]),
            responses::sse(vec![
                responses::ev_assistant_message("done", "finished"),
                responses::ev_completed("retry-response"),
            ]),
        ],
    )
    .await;

    test.submit_turn("Write the marker once, then finish")
        .await?;
    let requests = mock.requests();
    assert_eq!(requests.len(), 2);
    let original_input = requests[0].input();
    let retry_input = requests[1].input();
    let output = requests[1].function_call_output("write-marker");
    let mut expected_input = original_input;
    // The filter removes only a new contiguous reasoning suffix. A tool output
    // at the tail stops it, retaining the call, intervening reasoning and output.
    expected_input.extend([call["item"].clone(), reasoning["item"].clone(), output]);
    assert_eq!(normalize_outbound_items(retry_input), expected_input);
    assert_eq!(
        normalize_outbound_items(requests[1].inputs_of_type("function_call")),
        vec![call["item"].clone()],
    );
    assert_eq!(
        normalize_outbound_items(requests[1].inputs_of_type("reasoning")),
        vec![reasoning["item"].clone()],
    );
    let contents = test
        .fs()
        .read_file_text(
            &test.workspace_path_uri("retry-marker.txt")?,
            Default::default(),
            /*sandbox*/ None,
        )
        .await?;
    assert_eq!(contents.lines().collect::<Vec<_>>(), vec!["executed"]);

    test.codex.flush_rollout().await?;
    let (rollout, _, _) =
        RolloutRecorder::load_rollout_items(&test.codex.rollout_path().expect("rollout path"))
            .await?;
    let retained_reasoning = rollout
        .into_iter()
        .filter_map(|item| match item {
            RolloutItem::ResponseItem(envelope)
                if matches!(envelope.item, ResponseItem::Reasoning { .. }) =>
            {
                Some(without_turn_metadata(envelope.item))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        retained_reasoning,
        vec![serde_json::from_value::<ResponseItem>(
            reasoning["item"].clone()
        )?],
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retries_on_early_close() {
    skip_if_no_network!();

    let incomplete_sse = sse_incomplete();
    let completed_sse = responses::sse_completed("resp_ok");

    let (server, _) = start_streaming_sse_server(vec![
        vec![StreamingSseChunk {
            gate: None,
            body: incomplete_sse,
        }],
        vec![StreamingSseChunk {
            gate: None,
            body: completed_sse,
        }],
    ])
    .await;

    // Configure retry behavior explicitly to avoid mutating process-wide
    // environment variables.

    let model_provider = ModelProviderInfo {
        name: "openai".into(),
        base_url: Some(format!("{}/v1", server.uri())),
        model_catalog_url: None,
        // Environment variable that should exist in the test environment.
        // ModelClient will return an error if the environment variable for the
        // provider is not set.
        env_key: Some("PATH".into()),
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        gateway_oauth: None,
        aws: None,
        wire_api: WireApi::Responses,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        // exercise retry path: first attempt yields incomplete stream, so allow 1 retry
        request_max_retries: Some(0),
        stream_max_retries: Some(1),
        stream_idle_timeout_ms: Some(2000),
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        supports_standalone_web_search: false,
        capabilities: None,
        include_internal_metadata: false,
    };

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |config| {
            config.model_provider = model_provider;
        })
        .build_with_streaming_server(&server)
        .await
        .unwrap();

    codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hello".into(),
            text_elements: Vec::new(),
        }]))
        .await
        .unwrap();

    // Wait until TurnComplete (should succeed after retry).
    wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await;

    let requests = server.requests().await;
    assert_eq!(
        requests.len(),
        2,
        "expected retry after incomplete SSE stream"
    );

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connection_failure_pauses_retry_budget_until_provider_is_reachable() -> anyhow::Result<()>
{
    skip_if_no_network!(Ok(()));

    let bootstrap_server = responses::start_mock_server().await;
    let unavailable_listener = TcpListener::bind("127.0.0.1:0")?;
    let unavailable_address = unavailable_listener.local_addr()?;
    drop(unavailable_listener);

    let TestCodex { codex, .. } = test_codex()
        .with_config(move |config| {
            config.model_provider.base_url = Some(format!("http://{unavailable_address}/v1"));
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(1);
            config.model_provider.supports_websockets = false;
        })
        .build_with_auto_env(&bootstrap_server)
        .await?;

    codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "recover after the network returns".into(),
            text_elements: Vec::new(),
        }]))
        .await?;

    let EventMsg::StreamError(connection_error) =
        wait_for_event(&codex, |event| matches!(event, EventMsg::StreamError(_))).await
    else {
        unreachable!("predicate guarantees a stream error event");
    };
    assert_eq!(
        connection_error.message,
        "Reconnecting... waiting for network"
    );

    let recovered_server = MockServer::builder()
        .listener(TcpListener::bind(unavailable_address)?)
        .start()
        .await;
    let response_mock = responses::mount_sse_sequence(
        &recovered_server,
        vec![sse_incomplete(), responses::sse_completed("resp_recovered")],
    )
    .await;

    let EventMsg::StreamError(stream_error) =
        wait_for_event(&codex, |event| matches!(event, EventMsg::StreamError(_))).await
    else {
        unreachable!("predicate guarantees a stream error event");
    };
    assert_eq!(stream_error.message, "Reconnecting... 1/1");

    let EventMsg::TurnComplete(completed) =
        wait_for_event(&codex, |event| matches!(event, EventMsg::TurnComplete(_))).await
    else {
        unreachable!("predicate guarantees a turn complete event");
    };

    assert_eq!(completed.error, None);
    assert_eq!(response_mock.requests().len(), 2);

    Ok(())
}
