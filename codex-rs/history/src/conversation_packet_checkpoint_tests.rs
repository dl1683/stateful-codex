use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::CompactedItem;
use crate::ConversationInputCoverage;
use crate::ConversationPacketBoundary;
use crate::ConversationPacketBudget;
use crate::ConversationPacketInput;
use crate::ConversationPacketRecord;
use crate::ConversationPacketSize;
use crate::ConversationRecordKind;
use crate::RetainedSource;
use crate::RetainedSourceId;
use crate::RetainedSourceRole;
use crate::pack_conversation_packet;

#[test]
fn a_checkpoint_without_a_packet_keeps_the_legacy_shape() {
    let legacy = json!({ "message": "summary" });

    let item: CompactedItem = serde_json::from_value(legacy).expect("legacy checkpoint");
    let serialized = serde_json::to_value(&item).expect("serializes");

    assert_eq!(item.conversation_packet, None);
    assert_eq!(serialized.get("conversation_packet"), None);
}

#[test]
fn a_checkpoint_packet_round_trips_exactly() {
    let thread_id = ThreadId::from_u128(7);
    let answer = ConversationPacketRecord::new(
        thread_id,
        RetainedSource {
            id: RetainedSourceId {
                message_id: "m2".to_string(),
                turn_id: "turn-2".to_string(),
                role: RetainedSourceRole::Assistant,
            },
            revision: ResponseItemId::from_server("rev_2".to_string()),
            complete: true,
        },
        2,
        ConversationRecordKind::AssistantFinal,
        "(2, 1, 7) from research/HANDOFF.md:293".to_string(),
    );
    let packet = pack_conversation_packet(
        ConversationPacketInput {
            boundary: ConversationPacketBoundary::new(thread_id, 2),
            input_coverage: ConversationInputCoverage::CompleteThroughCutoff,
            previous: None,
            candidates: std::slice::from_ref(&answer),
        },
        ConversationPacketBudget {
            max_rendered_tokens: 2_048,
            max_rendered_bytes: 8_192,
            max_records: 64,
        },
        |_| ConversationPacketSize {
            rendered_tokens: 1,
            rendered_bytes: 1,
        },
    )
    .expect("packs");
    let mut item: CompactedItem =
        serde_json::from_value(json!({ "message": "summary" })).expect("checkpoint");
    item.conversation_packet = Some(packet);

    let restored: CompactedItem =
        serde_json::from_str(&serde_json::to_string(&item).expect("serializes"))
            .expect("deserializes");

    assert_eq!(restored, item);
}
