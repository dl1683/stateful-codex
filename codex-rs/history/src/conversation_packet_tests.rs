use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;

use super::*;
use crate::RetainedSourceId;

use ConversationInputCoverage::BoundedWindow;
use ConversationInputCoverage::CompleteThroughCutoff;
use ConversationRecordKind::AssistantCommentary;
use ConversationRecordKind::AssistantFinal;
use ConversationRecordKind::User;

const THREAD: u128 = 1;

fn record_with(
    thread: u128,
    message: &str,
    revision: &str,
    sequence: u64,
    kind: ConversationRecordKind,
    text: &str,
) -> ConversationPacketRecord {
    let source = RetainedSource {
        id: RetainedSourceId {
            message_id: message.to_string(),
            turn_id: format!("turn-{message}"),
            role: source_role(kind),
        },
        revision: ResponseItemId::from_server(revision.to_string()),
        complete: true,
    };
    ConversationPacketRecord::new(
        ThreadId::from_u128(thread),
        source,
        sequence,
        kind,
        text.to_string(),
    )
}

fn record(sequence: u64, kind: ConversationRecordKind, text: &str) -> ConversationPacketRecord {
    record_with(
        THREAD,
        &format!("m{sequence}"),
        &format!("rev_{sequence}"),
        sequence,
        kind,
        text,
    )
}

/// A renderer that frames each record and prints the coverage counters.
fn measure(packet: &ConversationPacket) -> ConversationPacketSize {
    let text: usize = packet
        .records()
        .iter()
        .map(|record| record.text().len())
        .sum();
    let coverage = packet.coverage();
    let counters = format!(
        "{}/{}",
        coverage.included_records(),
        coverage.omitted_by_budget()
    )
    .len();
    let bytes = 40 + counters + text + 20 * packet.records().len();
    ConversationPacketSize {
        rendered_tokens: bytes / 4,
        rendered_bytes: bytes,
    }
}

fn budget(bytes: usize, tokens: usize, records: usize) -> ConversationPacketBudget {
    ConversationPacketBudget {
        max_rendered_tokens: tokens,
        max_rendered_bytes: bytes,
        max_records: records,
    }
}

fn roomy() -> ConversationPacketBudget {
    budget(40_000, 10_000, 32)
}

fn pack(
    previous: Option<&ConversationPacket>,
    candidates: &[ConversationPacketRecord],
    budget: ConversationPacketBudget,
) -> Result<ConversationPacket, ConversationPacketError> {
    pack_with(99, CompleteThroughCutoff, previous, candidates, budget)
}

fn pack_with(
    through: u64,
    coverage: ConversationInputCoverage,
    previous: Option<&ConversationPacket>,
    candidates: &[ConversationPacketRecord],
    budget: ConversationPacketBudget,
) -> Result<ConversationPacket, ConversationPacketError> {
    let input = ConversationPacketInput {
        boundary: ConversationPacketBoundary::new(ThreadId::from_u128(THREAD), through),
        input_coverage: coverage,
        previous,
        candidates,
    };
    pack_conversation_packet(input, budget, measure)
}

fn sequences(packet: &ConversationPacket) -> Vec<u64> {
    packet
        .records()
        .iter()
        .map(ConversationPacketRecord::sequence)
        .collect()
}

#[test]
fn preserves_whole_messages_with_exact_values_and_sources() {
    let request = record(1, User, "Give me the three numbers that matter most.");
    let answer = record(
        2,
        AssistantFinal,
        "1. 20% chance the CANNOT survives (research/STEERING_ENDGAME.md:79)\n\
         2. 40% for a narrower CANNOT\n3. 20% both close; slack -0.0548 ns; 93,600 \u{b5}m\u{b2}",
    );
    let packet = pack_with(
        2,
        CompleteThroughCutoff,
        None,
        &[request.clone(), answer.clone()],
        roomy(),
    )
    .expect("packs");

    assert_eq!(
        packet,
        ConversationPacket {
            version: CONVERSATION_PACKET_VERSION,
            boundary: ConversationPacketBoundary::new(ThreadId::from_u128(THREAD), 2),
            records: vec![request, answer],
            coverage: ConversationPacketCoverage {
                input_coverage: CompleteThroughCutoff,
                considered_records: 2,
                included_records: 2,
                omitted_by_budget: 0,
                omitted_incomplete: 0,
            },
        }
    );
}

#[test]
fn deduplicates_by_source_revision_and_rejects_inconsistent_metadata() {
    let first = record_with(
        THREAD,
        "m2",
        "rev_a",
        2,
        AssistantFinal,
        "The value is 0.0230.",
    );
    let revised = record_with(
        THREAD,
        "m2",
        "rev_b",
        3,
        AssistantFinal,
        "The value is 0.0625.",
    );
    let other_thread = record_with(2, "m2", "rev_a", 2, AssistantFinal, "The value is 0.0230.");
    let previous = pack(None, std::slice::from_ref(&first), roomy()).expect("first packet");

    let packet = pack(
        Some(&previous),
        &[first.clone(), revised.clone(), other_thread.clone()],
        roomy(),
    )
    .expect("merged");
    assert_eq!(
        packet.records(),
        [first, other_thread, revised].as_slice()
    );

    let moved = record_with(
        THREAD,
        "m2",
        "rev_a",
        5,
        AssistantFinal,
        "The value is 0.0230.",
    );
    let reworded = record_with(
        THREAD,
        "m2",
        "rev_a",
        2,
        AssistantFinal,
        "The value is 0.9999.",
    );
    let rekinded = record_with(
        THREAD,
        "m2",
        "rev_a",
        2,
        AssistantCommentary,
        "The value is 0.0230.",
    );
    for inconsistent in [moved, reworded, rekinded] {
        let sequence = inconsistent.sequence();
        assert_eq!(
            pack(Some(&previous), &[inconsistent], roomy()),
            Err(ConversationPacketError::ConflictingSource { sequence })
        );
    }
}

#[test]
fn rejects_records_after_the_cutoff_or_with_the_wrong_role() {
    assert_eq!(
        pack_with(
            5,
            CompleteThroughCutoff,
            None,
            &[record(9, User, "Too late.")],
            roomy()
        ),
        Err(ConversationPacketError::AfterCutoff {
            sequence: 9,
            through_sequence: 5
        })
    );
    let assistant_as_user = ConversationPacketRecord::new(
        ThreadId::from_u128(THREAD),
        record(3, AssistantFinal, "x").source().clone(),
        3,
        User,
        "Mislabelled.".to_string(),
    );
    assert_eq!(
        pack(None, &[assistant_as_user], roomy()),
        Err(ConversationPacketError::InvalidSourceKind { sequence: 3 })
    );
}

fn run(
    previous: Option<&ConversationPacket>,
    candidates: &[ConversationPacketRecord],
    measure: impl FnMut(&ConversationPacket) -> ConversationPacketSize,
) -> Result<ConversationPacket, ConversationPacketError> {
    let input = ConversationPacketInput {
        boundary: ConversationPacketBoundary::new(ThreadId::from_u128(THREAD), 999),
        input_coverage: CompleteThroughCutoff,
        previous,
        candidates,
    };
    pack_conversation_packet(input, roomy(), measure)
}

fn never_measure(_: &ConversationPacket) -> ConversationPacketSize {
    panic!("oversized input was measured")
}

#[test]
fn enforces_input_bounds_before_measuring() {
    let many: Vec<ConversationPacketRecord> = (1..=MAX_PACKET_CANDIDATES as u64)
        .map(|n| record(n, User, "ok"))
        .collect();
    assert!(run(None, &many, measure).is_ok());
    let previous = pack(None, &[record(90, User, "carried")], roomy()).expect("previous");
    assert_eq!(
        run(Some(&previous), &many, never_measure),
        Err(ConversationPacketError::InputBoundExceeded {
            records: MAX_PACKET_CANDIDATES + 1,
            bytes: 2 * 64 + 7
        })
    );

    let half = MAX_PACKET_CANDIDATE_BYTES / 2;
    let exact = [
        record(1, User, &"a".repeat(half)),
        record(2, User, &"b".repeat(half)),
    ];
    assert!(run(None, &exact, measure).is_ok());
    let over = [
        record(1, User, &"a".repeat(half)),
        record(2, User, &"b".repeat(half + 1)),
    ];
    assert_eq!(
        run(None, &over, never_measure),
        Err(ConversationPacketError::InputBoundExceeded {
            records: 2,
            bytes: MAX_PACKET_CANDIDATE_BYTES + 1
        })
    );
}

#[test]
fn rejects_a_header_that_does_not_fit() {
    assert_eq!(
        pack(None, &[record(1, User, "hello")], budget(30, 10_000, 32)),
        Err(ConversationPacketError::HeaderExceedsBudget)
    );
}

#[test]
fn applies_priority_under_each_record_limit() {
    let history = [
        record(1, User, "first request"),
        record(2, AssistantFinal, "older answer"),
        record(3, User, "latest request"),
        record(4, AssistantFinal, "latest answer"),
        record(5, AssistantCommentary, "aside"),
        record(6, User, "message after the latest answer"),
    ];
    let expected: [&[u64]; 6] = [
        &[4],
        &[3, 4],
        &[2, 3, 4],
        &[2, 3, 4, 6],
        &[1, 2, 3, 4, 6],
        &[1, 2, 3, 4, 5, 6],
    ];
    for (limit, want) in expected.iter().enumerate() {
        let packet = pack(None, &history, budget(40_000, 10_000, limit + 1)).expect("packs");
        assert_eq!(
            sequences(&packet),
            want.to_vec(),
            "record limit {}",
            limit + 1
        );
        assert_eq!(
            packet.coverage().omitted_by_budget(),
            history.len() - want.len()
        );
    }
}

#[test]
fn token_and_byte_caps_bind_independently() {
    let history = [
        record(1, User, &"u".repeat(100)),
        record(2, AssistantFinal, &"f".repeat(100)),
    ];
    let full = pack(None, &history, roomy()).expect("packs");
    let full_size = measure(&full);
    let by_bytes = pack(
        None,
        &history,
        budget(full_size.rendered_bytes - 1, 10_000, 32),
    )
    .expect("packs");
    let by_tokens = pack(
        None,
        &history,
        budget(40_000, full_size.rendered_tokens - 1, 32),
    )
    .expect("packs");
    for packet in [&by_bytes, &by_tokens] {
        assert_eq!(sequences(packet), vec![2]);
    }
}

#[test]
fn the_returned_packet_fits_with_its_final_coverage() {
    let history: Vec<ConversationPacketRecord> = (1..=12)
        .map(|n| record(n, AssistantFinal, &"z".repeat(10)))
        .collect();
    for limit in 40..400 {
        let tight = budget(limit, 10_000, 64);
        if let Ok(packet) = pack_with(99, BoundedWindow, None, &history, tight) {
            let size = measure(&packet);
            assert!(
                size.rendered_bytes <= limit,
                "budget {limit} exceeded: {size:?}"
            );
            let coverage = packet.coverage();
            assert_eq!(
                coverage.included_records() + coverage.omitted_by_budget(),
                coverage.considered_records()
            );
            assert_eq!(coverage.input_coverage(), BoundedWindow);
        }
    }
}

#[test]
fn skips_incomplete_sources_without_truncating_others() {
    let mut shortened = record(1, User, "shortened original");
    shortened = ConversationPacketRecord::new(
        shortened.origin_thread_id(),
        RetainedSource {
            complete: false,
            ..shortened.source().clone()
        },
        1,
        User,
        shortened.text().to_string(),
    );
    let long = record(2, AssistantFinal, &"x".repeat(400));
    let short = record(3, AssistantFinal, "short answer");
    let packet = pack(
        None,
        &[shortened, long, short.clone()],
        budget(200, 10_000, 32),
    )
    .expect("packs");
    assert_eq!(packet.records(), [short].as_slice());
    assert_eq!(packet.coverage().omitted_incomplete(), 1);
    assert_eq!(packet.coverage().omitted_by_budget(), 1);
}

#[test]
fn carries_original_records_across_three_generations() {
    let answer = record(2, AssistantFinal, "(2, 1, 7) from research/HANDOFF.md:293");
    let first = pack(
        None,
        &[record(1, User, "What are the numbers?"), answer.clone()],
        roomy(),
    )
    .expect("first");
    let later = [
        record(3, User, "Now the ledger."),
        record(4, AssistantFinal, &"ledger ".repeat(6)),
    ];
    let tight = budget(262, 10_000, 3);
    let second = pack(Some(&first), &later, tight).expect("second");
    assert_eq!(pack(Some(&first), &later, tight).expect("repeat"), second);

    let third = pack(
        Some(&second),
        &[record(5, User, "And now?")],
        budget(262, 10_000, 3),
    )
    .expect("third");
    let carried = third
        .records()
        .iter()
        .find(|record| record.sequence() == 2)
        .expect("answer carried twice");
    assert_eq!(carried, &answer);
    assert!(second.coverage().omitted_by_budget() >= 1);
}
