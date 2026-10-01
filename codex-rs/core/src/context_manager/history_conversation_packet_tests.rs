use codex_history::CodexHarnessMetadata;
use codex_history::ResponseItemEnvelope;
use codex_protocol::ResponseItemId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;

use super::*;

fn answer(id: &str, order: u64, thread_id: ThreadId) -> ResponseItemEnvelope {
    ResponseItemEnvelope {
        item: ResponseItem::Message {
            id: Some(ResponseItemId::from_server(id.to_string())),
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: format!("answer {id}"),
            }],
            phase: Some(MessagePhase::FinalAnswer),
            internal_chat_message_metadata_passthrough: None,
        },
        metadata: Some(CodexHarnessMetadata {
            user_input_order: Some(order),
            conversation_origin_thread_id: Some(thread_id),
            ..Default::default()
        }),
    }
}

fn record(history: &mut ContextManager, id: &str, order: u64, thread_id: ThreadId) {
    history.record_annotated_items(
        &mut [answer(id, order, thread_id)],
        TruncationPolicy::Tokens(10_000),
    );
}

fn texts(packet: Option<Arc<ConversationPacket>>) -> Vec<String> {
    packet
        .iter()
        .flat_map(|packet| packet.records())
        .map(|record| record.text().to_string())
        .collect()
}

#[test]
fn capture_reads_the_unmodified_snapshot_and_installs_with_its_checkpoint() {
    let thread_id = ThreadId::new();
    let mut live = ContextManager::new();
    record(&mut live, "first", 0, thread_id);
    let mut input = live.clone();
    let capture = input.capture_conversation_packet(thread_id);
    // Later compactor edits to its own copy cannot change what was captured.
    input.remove_first_item();
    record(&mut live, "late", 1, thread_id);

    let installed = live.update_conversation_packet(capture);

    assert_eq!(texts(installed.clone()), vec!["answer first".to_string()]);
    assert_eq!(live.conversation_packet(), installed.as_ref());
}

#[test]
fn checkpoints_without_capture_carry_the_packet_forward() {
    let thread_id = ThreadId::new();
    let mut live = ContextManager::new();
    record(&mut live, "first", 0, thread_id);
    let capture = live.capture_conversation_packet(thread_id);
    let installed = live.update_conversation_packet(capture).expect("installed");

    let carried = live
        .update_conversation_packet(ConversationPacketUpdate::CarryForward)
        .expect("carried");

    assert!(Arc::ptr_eq(&installed, &carried));
}

#[test]
fn stale_capture_after_a_reset_installs_nothing() {
    let thread_id = ThreadId::new();
    let mut live = ContextManager::new();
    record(&mut live, "first", 0, thread_id);
    let capture = live.clone().capture_conversation_packet(thread_id);
    live.replace_annotated(Vec::new());

    assert_eq!(live.update_conversation_packet(capture), None);
}

#[test]
fn destructive_replacement_clears_the_packet() {
    let thread_id = ThreadId::new();
    let mut live = ContextManager::new();
    record(&mut live, "first", 0, thread_id);
    let capture = live.capture_conversation_packet(thread_id);
    live.update_conversation_packet(capture).expect("installed");

    live.replace_annotated(vec![answer("kept", 1, thread_id)]);

    assert_eq!(live.conversation_packet(), None);
}
