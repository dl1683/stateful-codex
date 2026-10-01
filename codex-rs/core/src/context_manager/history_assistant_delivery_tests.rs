use codex_history::CodexHarnessMetadata;
use codex_history::ConversationRecordKind;
use codex_history::ResponseItemEnvelope;
use codex_protocol::ResponseItemId;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;

use super::*;

fn delivery(id: &str, order: u64, thread_id: ThreadId) -> ResponseItemEnvelope {
    ResponseItemEnvelope {
        item: ResponseItem::Message {
            id: Some(ResponseItemId::from_server(id.to_string())),
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: format!("text {id}"),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        },
        metadata: Some(CodexHarnessMetadata {
            user_input_order: Some(order),
            conversation_origin_thread_id: Some(thread_id),
            assistant_delivery_classification: Some(AssistantDeliveryClassification::Pending),
            ..Default::default()
        }),
    }
}

fn recorded(ids: &[&str], thread_id: ThreadId) -> ContextManager {
    let mut history = ContextManager::new();
    for (order, id) in (0..).zip(ids) {
        history.record_annotated_items(
            &mut [delivery(id, order, thread_id)],
            TruncationPolicy::Tokens(10_000),
        );
    }
    history
}

fn resolve(
    history: &mut ContextManager,
    delivered: &[(&str, &str)],
    completion: ResponseCompletion,
) {
    let candidates = delivered
        .iter()
        .filter_map(|(id, text)| history.assistant_delivery_candidate(id, text.to_string()))
        .collect();
    for event in classify_completed_response(candidates, completion) {
        assert!(history.record_retained_context(&event));
    }
}

fn packet_kinds(
    history: &ContextManager,
    thread_id: ThreadId,
) -> Vec<(String, ConversationRecordKind)> {
    let mut history = history.clone();
    let capture = history.capture_conversation_packet(thread_id);
    let packet = history.update_conversation_packet(capture);
    packet
        .iter()
        .flat_map(|packet| packet.records())
        .map(|record| (record.text().to_string(), record.kind()))
        .collect()
}

fn envelope_states(
    history: &ContextManager,
) -> Vec<(
    Option<RetainedSource>,
    Option<AssistantDeliveryClassification>,
)> {
    history
        .annotated_items()
        .iter()
        .map(|envelope| {
            let metadata = envelope.metadata.as_ref();
            (
                metadata.and_then(|metadata| metadata.retained_source.clone()),
                metadata.and_then(|metadata| metadata.assistant_delivery_classification),
            )
        })
        .collect()
}

#[test]
fn resolution_keeps_identity_and_unresolved_or_rewritten_deliveries_stay_out_of_packets() {
    use AssistantDeliveryClassification::Commentary;
    use AssistantDeliveryClassification::Final;
    use AssistantDeliveryClassification::Pending;
    let thread_id = ThreadId::new();
    let ids = [
        "tool-preamble",
        "preamble",
        "answer",
        "rewritten",
        "interrupted",
    ];
    let mut history = recorded(&ids, thread_id);
    let sources = envelope_states(&history)
        .into_iter()
        .map(|(source, _)| source);
    // Nothing is resolved yet, so nothing may enter a packet under a kind it could lose.
    assert_eq!(packet_kinds(&history, thread_id), Vec::new());

    resolve(
        &mut history,
        &[("tool-preamble", "text tool-preamble")],
        ResponseCompletion::Continues,
    );
    resolve(
        &mut history,
        &[("preamble", "text preamble"), ("answer", "text answer")],
        ResponseCompletion::Answered,
    );
    // A delivery the user saw with different text is never resolved, even when last.
    resolve(
        &mut history,
        &[("rewritten", "visible text")],
        ResponseCompletion::Answered,
    );

    assert_eq!(
        packet_kinds(&history, thread_id),
        vec![
            (
                "text tool-preamble".to_string(),
                ConversationRecordKind::AssistantCommentary
            ),
            (
                "text preamble".to_string(),
                ConversationRecordKind::AssistantCommentary
            ),
            (
                "text answer".to_string(),
                ConversationRecordKind::AssistantFinal
            ),
        ]
    );
    assert_eq!(
        envelope_states(&history),
        sources
            .zip([
                Some(Commentary),
                Some(Commentary),
                Some(Final),
                Some(Pending),
                Some(Pending),
            ])
            .collect::<Vec<_>>()
    );
}
