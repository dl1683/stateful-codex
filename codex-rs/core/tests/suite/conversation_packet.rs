//! Remote-v2 compaction persists the exact original deliveries with each checkpoint.

use anyhow::Context;
use anyhow::Result;
use codex_history::ConversationPacket;
use codex_history::ConversationRecordKind;
use codex_history::RolloutItem;
use codex_login::CodexAuth;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use core_test_support::responses;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;

const FIRST_REQUEST: &str = "How many config files are there, and where is the limit?";
const FIRST_ANSWER: &str = "There are 42 config files; the limit is 8192 in src/config.rs:118.";
const SECOND_REQUEST: &str = "Which test covers that limit?";
const SECOND_ANSWER: &str = "tests/limits.rs:27 covers the 8192 limit.";

fn final_answer(id: &str, text: &str) -> Value {
    json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "role": "assistant",
            "id": id,
            "phase": "final_answer",
            "content": [{"type": "output_text", "text": text}],
        },
    })
}

fn compaction(summary: &str) -> String {
    sse(vec![
        json!({
            "type": "response.output_item.done",
            "item": {"type": "compaction", "encrypted_content": summary},
        }),
        responses::ev_completed("compact-response"),
    ])
}

fn persisted_packets(path: &Path) -> Result<Vec<ConversationPacket>> {
    fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(codex_rollout::parse_rollout_line)
        .filter_map(|line| match line {
            Ok(line) => match line.item {
                RolloutItem::Compacted(compacted) => Some(
                    compacted
                        .conversation_packet
                        .context("checkpoint without a conversation packet"),
                ),
                _ => None,
            },
            Err(error) => Some(Err(error.into())),
        })
        .collect()
}

fn summary(packet: &ConversationPacket) -> Vec<(ConversationRecordKind, &str)> {
    packet
        .records()
        .iter()
        .map(|record| (record.kind(), record.text()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_compact_v2_persists_original_deliveries_across_two_checkpoints() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = wiremock::MockServer::start().await;
    let response_mock = responses::mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                final_answer("msg-first", FIRST_ANSWER),
                responses::ev_completed("response-1"),
            ]),
            compaction("FIRST_OPAQUE_SUMMARY"),
            sse(vec![
                final_answer("msg-second", SECOND_ANSWER),
                responses::ev_completed("response-2"),
            ]),
            compaction("SECOND_OPAQUE_SUMMARY"),
        ],
    )
    .await;
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .build(&server)
        .await?;

    test.submit_turn(FIRST_REQUEST).await?;
    test.codex.submit(Op::Compact).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    test.submit_turn(SECOND_REQUEST).await?;
    test.codex.submit(Op::Compact).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let rollout_path = test
        .session_configured
        .rollout_path
        .clone()
        .context("rollout path")?;
    test.codex.shutdown_and_wait().await?;

    let packets = persisted_packets(&rollout_path)?;
    let [first, second] = packets.as_slice() else {
        panic!("expected two checkpoints, got {}", packets.len());
    };
    // Only original deliveries are records: no opaque summary, trigger or context fragment.
    assert_eq!(
        summary(first),
        vec![
            (ConversationRecordKind::User, FIRST_REQUEST),
            (ConversationRecordKind::AssistantFinal, FIRST_ANSWER),
        ]
    );
    assert_eq!(
        summary(second),
        vec![
            (ConversationRecordKind::User, FIRST_REQUEST),
            (ConversationRecordKind::AssistantFinal, FIRST_ANSWER),
            (ConversationRecordKind::User, SECOND_REQUEST),
            (ConversationRecordKind::AssistantFinal, SECOND_ANSWER),
        ]
    );
    // The original answer keeps its identity, revision, sequence and text.
    assert_eq!(first.records()[..2], second.records()[..2]);
    assert!(second.boundary().through_sequence() > first.boundary().through_sequence());

    let requests = response_mock.requests();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        let body = request.body_json().to_string();
        assert!(
            !body.contains("conversation_packet") && !body.contains("conversation_origin"),
            "packet sidecar metadata reached provider input: {body}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phase_less_deliveries_are_resolved_before_the_next_checkpoint() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = wiremock::MockServer::start().await;
    let mut continued = responses::ev_completed("response-3");
    continued["response"]["end_turn"] = json!(false);
    responses::mount_sse_sequence(
        &server,
        vec![
            // A preamble before a tool call is commentary; the answer after it is final.
            sse(vec![
                responses::ev_assistant_message("msg-preamble", "Checking the config."),
                responses::ev_function_call("call-1", "missing_tool", "{}"),
                responses::ev_completed("response-1"),
            ]),
            sse(vec![
                responses::ev_assistant_message("msg-answer", FIRST_ANSWER),
                responses::ev_completed("response-2"),
            ]),
            compaction("FIRST_OPAQUE_SUMMARY"),
            // A response the provider continues cannot have delivered the answer.
            sse(vec![
                responses::ev_assistant_message("msg-continued", "Still looking."),
                continued,
            ]),
            sse(vec![
                responses::ev_assistant_message("msg-progress", "Found it."),
                responses::ev_assistant_message("msg-second", SECOND_ANSWER),
                responses::ev_completed("response-4"),
            ]),
            compaction("SECOND_OPAQUE_SUMMARY"),
        ],
    )
    .await;
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .build(&server)
        .await?;

    for request in [FIRST_REQUEST, SECOND_REQUEST] {
        test.submit_turn(request).await?;
        test.codex.submit(Op::Compact).await?;
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_))
        })
        .await;
    }
    let rollout_path = test
        .session_configured
        .rollout_path
        .clone()
        .context("rollout path")?;
    test.codex.shutdown_and_wait().await?;

    let packets = persisted_packets(&rollout_path)?;
    let [_, second] = packets.as_slice() else {
        panic!("expected two checkpoints, got {}", packets.len());
    };
    assert_eq!(
        summary(second),
        vec![
            (ConversationRecordKind::User, FIRST_REQUEST),
            (
                ConversationRecordKind::AssistantCommentary,
                "Checking the config."
            ),
            (ConversationRecordKind::AssistantFinal, FIRST_ANSWER),
            (ConversationRecordKind::User, SECOND_REQUEST),
            (
                ConversationRecordKind::AssistantCommentary,
                "Still looking."
            ),
            (ConversationRecordKind::AssistantCommentary, "Found it."),
            (ConversationRecordKind::AssistantFinal, SECOND_ANSWER),
        ]
    );
    Ok(())
}
