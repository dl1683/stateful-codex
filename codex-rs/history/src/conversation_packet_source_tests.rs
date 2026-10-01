use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;

use super::*;
use crate::CodexHarnessMetadata;
use crate::RetainedContext;
use crate::RetainedInputSource;
use crate::RetainedSourceId;
use crate::RetainedUserMessage;
use crate::UserInputOrigin;

use ConversationInputCoverage::BoundedWindow;
use ConversationInputCoverage::CompleteThroughCutoff;
use ConversationInputCoverage::Unknown;
use ConversationRecordKind::AssistantCommentary;
use ConversationRecordKind::AssistantFinal;
use ConversationRecordKind::User;
use HistoryContinuity::Rewritten;
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
    assemble(history, &RetainedContext::default(), None, SinceThreadStart)
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

fn assemble(
    history: &[ResponseItemEnvelope],
    retained: &RetainedContext,
    previous: Option<&ConversationPacket>,
    continuity: HistoryContinuity,
) -> ConversationPacket {
    let thread_id = thread();
    assemble_conversation_packet(ConversationPacketSource {
        thread_id,
        history,
        retained,
        previous,
        continuity,
    })
    .expect("assembled")
    .expect("packet")
}

#[test]
fn duplicate_delivery_evidence_is_packed_once() {
    let mut retained = RetainedContext::default();
    let mut retain = |id: &str, order, text: &str, phase| {
        let message = RetainedUserMessage {
            turn_id: "turn".to_string(),
            message_id: Some(id.to_string()),
            text: text.to_string(),
            complete: true,
            origin: UserInputOrigin::User,
            phase,
            origin_thread_id: Some(thread()),
        };
        let local = RetainedInputSource::Local(Some(order));
        retained
            .record_assistant_message(message, local)
            .expect("source")
    };
    let answer_source = retain("final", 1, "Done", Some(MessagePhase::FinalAnswer));
    // A confirmed Code Mode send exists only as retained evidence and has no message phase.
    let send_source = retain("call-1", 2, "Sent: 3 files", None);
    let history = [edited(
        delivered("final", 1, AssistantFinal, "Done"),
        |metadata| {
            metadata.retained_source = Some(answer_source.clone());
        },
    )];

    let first = assemble(&history, &retained, None, SinceThreadStart);
    let second = assemble(&history, &retained, Some(&first), Rewritten);

    let records = [
        ConversationPacketRecord::new(thread(), answer_source, 1, AssistantFinal, "Done".into()),
        ConversationPacketRecord::new(
            thread(),
            send_source,
            2,
            AssistantCommentary,
            "Sent: 3 files".into(),
        ),
    ];
    assert_eq!(first.records(), &records);
    assert_eq!(coverage(&first), (CompleteThroughCutoff, 2, 2, 0, 0));
    assert_eq!(second.records(), &records);
    assert_eq!(coverage(&second), (BoundedWindow, 2, 2, 0, 0));
}

#[test]
fn bounded_window_keeps_room_for_fresh_deliveries() {
    let early: Vec<_> = (0..40)
        .map(|order| delivered(&format!("e{order}"), order, User, "x"))
        .collect();
    let previous = first_packet(&early);
    assert!(previous.records().len() > FRESH_RESERVE);
    let fresh: Vec<_> = (40..120)
        .map(|order| delivered(&format!("late{order}"), order, User, "y"))
        .chain([delivered("final", 120, AssistantFinal, "Answer: 120")])
        .collect();

    let packet = assemble(
        &fresh,
        &RetainedContext::default(),
        Some(&previous),
        Rewritten,
    );

    // The full previous packet yields offered slots to the newest fresh deliveries.
    let (input, considered, ..) = coverage(&packet);
    assert_eq!((input, considered), (BoundedWindow, MAX_PACKET_CANDIDATES));
    assert_eq!(
        packet.boundary(),
        ConversationPacketBoundary::new(thread(), 120)
    );
    assert!(
        packet
            .records()
            .contains(&record("final", 120, AssistantFinal, "Answer: 120"))
    );
}

#[test]
fn carried_finals_outrank_fresh_commentary() {
    // A previous packet larger than the fresh reserve, with its finals the oldest records.
    let requests = (6..40).map(|order| delivered(&format!("u{order}"), order, User, "?"));
    let previous = first_packet(
        &seed_finals()
            .into_iter()
            .chain(requests)
            .collect::<Vec<_>>(),
    );
    assert!(previous.records().len() > FRESH_RESERVE);
    assert_eq!(finals_in(&previous), vec![0, 1, 2, 3, 4, 5]);
    let fresh: Vec<_> = commentary(40..110).collect();

    let packet = assemble(
        &fresh,
        &RetainedContext::default(),
        Some(&previous),
        Rewritten,
    );

    assert_eq!(finals_in(&packet), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn cutoff_is_the_latest_captured_delivery() {
    let retained = RetainedContext::default();
    let previous = first_packet(&[delivered("request", 5, User, "first")]);

    let carried = assemble(&[], &retained, Some(&previous), Rewritten);
    let later = [delivered("later", 7, User, "second")];
    let extended = assemble(&later, &retained, Some(&previous), Rewritten);

    assert_eq!(carried.boundary(), previous.boundary());
    assert_eq!(carried.records(), previous.records());
    assert_eq!(
        extended.boundary(),
        ConversationPacketBoundary::new(thread(), 7)
    );
    let records = [
        record("request", 5, User, "first"),
        record("later", 7, User, "second"),
    ];
    assert_eq!(extended.records(), &records);
    let empty = ConversationPacketSource {
        thread_id: thread(),
        history: &[],
        retained: &retained,
        previous: None,
        continuity: SinceThreadStart,
    };
    assert_eq!(assemble_conversation_packet(empty), Ok(None));
}

#[test]
fn final_evicted_from_retained_buffer_is_still_captured_from_history() {
    let mut retained = RetainedContext::default();
    let mut retain = |id: &str, order, phase| {
        let message = RetainedUserMessage {
            turn_id: "turn".to_string(),
            message_id: Some(id.to_string()),
            text: if id == "final" { "Answer: 42" } else { "." }.to_string(),
            complete: true,
            origin: UserInputOrigin::User,
            phase: Some(phase),
            origin_thread_id: Some(thread()),
        };
        retained.record_assistant_message(message, RetainedInputSource::Local(Some(order)))
    };
    let final_source = retain("final", 0, MessagePhase::FinalAnswer).expect("final source");
    for order in 1..12 {
        retain(&format!("c{order}"), order, MessagePhase::Commentary);
    }
    // The final's retained record was evicted; its annotated original keeps the same source.
    assert!(retained.has_omitted_assistant_messages());
    assert!(
        retained
            .ordered_entries()
            .all(|(_, entry)| retained.source(entry).as_ref() != Some(&final_source))
    );
    let history = [edited(
        delivered("final", 0, AssistantFinal, "Answer: 42"),
        |metadata| {
            metadata.retained_source = Some(final_source.clone());
        },
    )];

    let packet = assemble(&history, &retained, None, SinceThreadStart);

    let original = ConversationPacketRecord::new(
        thread(),
        final_source,
        0,
        AssistantFinal,
        "Answer: 42".into(),
    );
    assert!(packet.records().contains(&original));
    assert_eq!(coverage(&packet).0, BoundedWindow);
}
