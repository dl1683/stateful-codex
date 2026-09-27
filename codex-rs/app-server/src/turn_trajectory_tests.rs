use super::*;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::protocol::RawResponseCompletedEvent;
use codex_protocol::protocol::TokenUsage;

#[test]
fn counts_only_outputs_paired_with_observed_model_calls() {
    let mut state = TurnTrajectoryState::default();
    state.record_response_item(&ResponseItem::FunctionCall {
        id: None,
        name: "example".to_string(),
        namespace: None,
        arguments: "{}".to_string(),
        encrypted_function_args: None,
        call_id: "paired".to_string(),
        internal_chat_message_metadata_passthrough: None,
    });
    state.record_response_item(&ResponseItem::FunctionCallOutput {
        id: None,
        call_id: None,
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload::from_text("injected".to_string()),
        internal_chat_message_metadata_passthrough: None,
    });
    state.record_response_item(&ResponseItem::FunctionCallOutput {
        id: None,
        call_id: Some("paired".to_string()),
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload::from_text("ok".to_string()),
        internal_chat_message_metadata_passthrough: None,
    });

    assert_eq!(
        state.trajectory,
        TurnTrajectory {
            model_tool_calls: 1,
            model_function_tool_calls: 1,
            tool_output_bytes: 4,
            ..Default::default()
        }
    );
}

#[test]
fn accumulates_exact_usage_from_completed_responses() {
    let mut state = TurnTrajectoryState::default();
    let first = EventMsg::RawResponseCompleted(RawResponseCompletedEvent {
        response_id: "response-1".to_string(),
        token_usage: Some(TokenUsage {
            input_tokens: 100,
            cached_input_tokens: 80,
            cache_write_input_tokens: 5,
            output_tokens: 20,
            reasoning_output_tokens: 7,
            total_tokens: 120,
            codex_rollout_budget_units: None,
        }),
        usage_metadata: None,
    });
    let second = EventMsg::RawResponseCompleted(RawResponseCompletedEvent {
        response_id: "response-2".to_string(),
        token_usage: Some(TokenUsage {
            input_tokens: 60,
            cached_input_tokens: 40,
            cache_write_input_tokens: 0,
            output_tokens: 10,
            reasoning_output_tokens: 3,
            total_tokens: 70,
            codex_rollout_budget_units: None,
        }),
        usage_metadata: None,
    });

    state.observe("turn-1", &first);
    let snapshot = state
        .observe("turn-1", &second)
        .expect("completed response emits a snapshot");

    assert_eq!(
        snapshot,
        TurnTrajectorySnapshot {
            trajectory: TurnTrajectory {
                completed_model_responses: 2,
                ..Default::default()
            },
            token_usage: Some(StatefulTokenUsage {
                total_tokens: 190,
                input_tokens: 160,
                cached_input_tokens: 120,
                cache_write_input_tokens: 5,
                output_tokens: 30,
                reasoning_output_tokens: 10,
            }),
        }
    );
}
