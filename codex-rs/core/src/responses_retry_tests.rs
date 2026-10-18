use super::ResponsesStreamRequest;
use super::ResponsesStreamRetryState;
use super::handle_response_stream_error;
use super::log_retry;
use super::remove_interrupted_reasoning;
use crate::realtime_history::RealtimeHistoryState;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing_test::internal::MockWriter;

#[test]
fn retry_reasoning_filter_preserves_original_items_and_non_reasoning_tails() {
    let earlier = json!({"type": "reasoning", "id": "earlier", "summary": []});
    let interrupted = json!({"type": "reasoning", "id": "interrupted", "summary": []});
    let user = json!({"type": "message", "role": "user", "content": []});
    let assistant = json!({"type": "message", "role": "assistant", "content": []});
    let call = json!({
        "type": "function_call", "name": "exec_command", "call_id": "call", "arguments": "{}"
    });
    let output = json!({"type": "function_call_output", "call_id": "call", "output": "done"});
    let original = vec![user.clone(), earlier.clone()];
    let mut cases = vec![
        (vec![], vec![]),
        (original.clone(), original.clone()),
        (
            vec![user, earlier.clone(), interrupted.clone()],
            original.clone(),
        ),
        // History normalization may move original items; membership, rather
        // than the original length, protects earlier reasoning at the tail.
        (
            vec![earlier.clone(), interrupted.clone()],
            vec![earlier.clone()],
        ),
    ];
    for tail in [assistant, call, output] {
        let retained = vec![earlier.clone(), interrupted.clone(), tail];
        cases.push((retained.clone(), retained.clone()));
        let mut with_reasoning = retained.clone();
        with_reasoning.extend([interrupted.clone(), interrupted.clone()]);
        cases.push((with_reasoning, retained));
    }
    let original = original
        .into_iter()
        .map(serde_json::from_value::<ResponseItem>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for (input, expected) in cases {
        let mut input = input
            .into_iter()
            .map(serde_json::from_value::<ResponseItem>)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let expected = expected
            .into_iter()
            .map(serde_json::from_value::<ResponseItem>)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        remove_interrupted_reasoning(&mut input, &original);
        assert_eq!(input, expected);
    }
}

#[tokio::test]
async fn sampling_retry_logs_stream_error_context() {
    let (_session, turn_context) = make_session_and_context().await;
    let buffer: &'static std::sync::Mutex<Vec<u8>> =
        Box::leak(Box::new(std::sync::Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(MockWriter::new(buffer))
        .finish();
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);

    log_retry(
        ResponsesStreamRequest::Sampling,
        &turn_context,
        &CodexErr::Stream("websocket closed by server before response.completed".to_string()),
        /*retries*/ 2,
        /*max_retries*/ 5,
        Duration::from_secs(1),
    );

    let logs = String::from_utf8(
        buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
    )
    .expect("retry log should be valid utf-8");
    assert!(logs.contains("stream disconnected - retrying sampling request"));
    assert!(logs.contains(&format!("turn_id={}", turn_context.sub_id)));
    assert!(logs.contains("retries=2"));
    assert!(logs.contains("max_retries=5"));
    assert!(logs.contains(
        "sampling_error=stream disconnected before completion: websocket closed by server before response.completed"
    ));
}

/// Time spent reporting a retry must count toward the server's original deadline.
#[tokio::test]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "test holds the event-delivery lock to delay notification while virtual time advances"
)]
async fn stream_retry_preserves_deadline_across_delayed_notification() {
    let (mut session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    session.realtime_history = Some(Mutex::new(RealtimeHistoryState::default()));
    let mut client_session = session.services.model_client.new_session();
    // The second retry emits a notification even when release builds hide the first.
    let mut retry_state = ResponsesStreamRetryState {
        retries: 1,
        ..Default::default()
    };

    tokio::time::pause();
    let advice = RetryAfter::from_delay(Duration::from_secs(10)).expect("retry deadline");
    tokio::time::advance(Duration::from_secs(4)).await;

    // Hold the event-delivery lock so reporting the error consumes part of the deadline.
    let history_guard = session.realtime_history.as_ref().unwrap().lock().await;
    let retry = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 2,
        CodexErr::InternalServerError.with_retry_after(advice),
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(4)).await;
    drop(history_guard);

    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(futures::poll!(&mut retry).is_pending());
    retry.await.expect("retry should be allowed");
    // Tokio rounds timer deadlines up to the next millisecond.
    let resumed_at = Instant::now();
    assert!(
        (advice.deadline()..=advice.deadline() + Duration::from_millis(1)).contains(&resumed_at),
        "retry resumed at {resumed_at:?}, expected {advice:?}"
    );
}
