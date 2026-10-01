use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

use super::*;
use crate::CodexHarnessMetadata;
use crate::RetainedSourceId;

use ConversationInputCoverage::BoundedWindow;
use ConversationInputCoverage::CompleteThroughCutoff;
use ConversationInputCoverage::Unknown;
use ConversationRecordKind::AssistantCommentary;
use ConversationRecordKind::AssistantFinal;
use ConversationRecordKind::User;
use HistoryContinuity::SinceThreadStart;

fn thread() -> ThreadId {
    ThreadId::from_u128(7)
}

fn source(id: &str, kind: ConversationRecordKind) -> RetainedSource {
    let role = match kind {
        User => RetainedSourceRole::User,
        AssistantFinal | AssistantCommentary => RetainedSourceRole::Assistant,
    };
    let (message_id, turn_id) = (id.to_string(), "turn".to_string());
    RetainedSource {
        id: RetainedSourceId {
            message_id,
            turn_id,
            role,
        },
        revision: ResponseItemId::from_server(format!("rev_{id}")),
        complete: true,
    }
}

fn record(
    id: &str,
    order: u64,
    kind: ConversationRecordKind,
    text: &str,
) -> ConversationPacketRecord {
    ConversationPacketRecord::new(thread(), source(id, kind), order, kind, text.to_string())
}

/// A host-recorded message with its retained source, delivery order and origin.
fn delivered(
    id: &str,
    order: u64,
    kind: ConversationRecordKind,
    text: &str,
) -> ResponseItemEnvelope {
    let text = text.to_string();
    let (role, content, phase) = match kind {
        User => ("user", ContentItem::InputText { text }, None),
        AssistantFinal => (
            "assistant",
            ContentItem::OutputText { text },
            Some(MessagePhase::FinalAnswer),
        ),
        AssistantCommentary => ("assistant", ContentItem::OutputText { text }, None),
    };
    ResponseItemEnvelope {
        item: ResponseItem::Message {
            id: Some(ResponseItemId::from_server(id.to_string())),
            role: role.to_string(),
            content: vec![content],
            phase,
            internal_chat_message_metadata_passthrough: None,
        },
        metadata: Some(CodexHarnessMetadata {
            retained_source: Some(source(id, kind)),
            user_input_order: Some(order),
            conversation_origin_thread_id: Some(thread()),
            ..Default::default()
        }),
    }
}

fn edited(
    mut envelope: ResponseItemEnvelope,
    edit: impl FnOnce(&mut CodexHarnessMetadata),
) -> ResponseItemEnvelope {
    edit(envelope.metadata.get_or_insert_default());
    envelope
}

fn first_packet(history: &[ResponseItemEnvelope]) -> ConversationPacket {
    let thread_id = thread();
    let continuity = SinceThreadStart;
    let source = ConversationPacketSource {
        thread_id,
        history,
        continuity,
    };
    assemble_conversation_packet(source)
        .expect("assembled")
        .expect("packet")
}

/// (input coverage, considered, included, omitted by budget, omitted incomplete)
fn coverage(
    packet: &ConversationPacket,
) -> (ConversationInputCoverage, usize, usize, usize, usize) {
    let coverage = packet.coverage();
    (
        coverage.input_coverage(),
        coverage.considered_records(),
        coverage.included_records(),
        coverage.omitted_by_budget(),
        coverage.omitted_incomplete(),
    )
}

fn seed_finals() -> Vec<ResponseItemEnvelope> {
    (0..6)
        .map(|order| delivered(&format!("f{order}"), order, AssistantFinal, "seed"))
        .collect()
}

fn commentary(orders: std::ops::Range<u64>) -> impl Iterator<Item = ResponseItemEnvelope> {
    orders.map(|order| delivered(&format!("c{order}"), order, AssistantCommentary, "."))
}

fn finals_in(packet: &ConversationPacket) -> Vec<u64> {
    let finals = packet
        .records()
        .iter()
        .filter(|record| record.kind() == AssistantFinal);
    finals.map(ConversationPacketRecord::sequence).collect()
}

#[test]
fn packs_original_deliveries_by_delivery_order_and_excludes_compaction_output() {
    let answer = "Total: 42 in src/lib.rs:10";
    let summary = edited(
        delivered("summary", 9, AssistantFinal, "summary"),
        |metadata| {
            metadata.compaction_output = true;
        },
    );
    let context = ResponseItemEnvelope::new(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "<environment_context/>".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    });

    // Vector position differs from delivery order; only the order is used.
    let packet = first_packet(&[
        delivered("final", 3, AssistantFinal, answer),
        delivered("request", 2, User, "Count the items"),
        context,
        ResponseItemEnvelope::new(ResponseItem::CompactionTrigger {}),
        summary,
    ]);

    let records = [
        record("request", 2, User, "Count the items"),
        record("final", 3, AssistantFinal, answer),
    ];
    assert_eq!(packet.records(), &records);
    assert_eq!(
        packet.boundary(),
        ConversationPacketBoundary::new(thread(), 3)
    );
    assert_eq!(coverage(&packet), (CompleteThroughCutoff, 2, 2, 0, 0));
}

#[test]
fn missing_provenance_is_omitted_and_reported_as_unknown() {
    let unstamped = edited(delivered("unstamped", 1, User, "no origin"), |metadata| {
        metadata.conversation_origin_thread_id = None;
    });
    let unclassified = edited(delivered("bare", 2, AssistantCommentary, "?"), |metadata| {
        *metadata = CodexHarnessMetadata::default();
    });

    let packet = first_packet(&[
        unstamped,
        unclassified,
        delivered("request", 3, User, "kept"),
    ]);

    assert_eq!(packet.records(), &[record("request", 3, User, "kept")]);
    assert_eq!(coverage(&packet), (Unknown, 1, 1, 0, 0));
}

#[test]
fn incomplete_sources_are_counted_and_lower_coverage() {
    let partial = edited(
        delivered("partial", 1, User, "beside an image"),
        |metadata| {
            if let Some(source) = &mut metadata.retained_source {
                source.complete = false;
            }
        },
    );

    let packet = first_packet(&[partial, delivered("request", 2, User, "kept")]);

    assert_eq!(packet.records(), &[record("request", 2, User, "kept")]);
    assert_eq!(coverage(&packet), (BoundedWindow, 1, 1, 0, 1));
}

#[test]
fn older_finals_outrank_later_commentary_in_the_window() {
    let history: Vec<_> = seed_finals().into_iter().chain(commentary(6..76)).collect();

    let packet = first_packet(&history);

    assert_eq!(finals_in(&packet), vec![0, 1, 2, 3, 4, 5]);
    let (input, considered, ..) = coverage(&packet);
    assert_eq!((input, considered), (BoundedWindow, MAX_PACKET_CANDIDATES));
}
