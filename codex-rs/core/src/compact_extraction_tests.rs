use super::*;
use codex_history::CodexHarnessMetadata;
use codex_history::RetainedSource;
use codex_history::RetainedSourceId;
use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;

const OPENING: &str = "<capsule_sidecar_v1 request=\"m-1\">";

fn message(role: &str, texts: &[&str]) -> ResponseItem {
    let content = texts
        .iter()
        .map(|text| match role {
            "assistant" => ContentItem::OutputText {
                text: (*text).to_string(),
            },
            _ => ContentItem::InputText {
                text: (*text).to_string(),
            },
        })
        .collect();
    ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content,
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

fn sourced(
    item: ResponseItem,
    role: RetainedSourceRole,
    revision: &str,
    order: u64,
) -> ResponseItemEnvelope {
    ResponseItemEnvelope {
        item,
        metadata: Some(CodexHarnessMetadata {
            retained_source: Some(RetainedSource {
                id: RetainedSourceId {
                    message_id: format!("msg-{revision}"),
                    turn_id: "turn-1".to_string(),
                    role,
                },
                revision: ResponseItemId::from_server(revision.to_string()),
                complete: true,
            }),
            user_input_order: Some(order),
            ..Default::default()
        }),
    }
}

fn origin(revision: &str, order: u64, byte_identical: bool) -> Option<CompactionSourceOrigin> {
    Some(CompactionSourceOrigin {
        item_id: format!("msg-{revision}"),
        turn_id: "turn-1".to_string(),
        origin_thread_id: None,
        sequence: Some(order),
        source_revision: revision.to_string(),
        byte_identical,
    })
}

fn part(
    item_index: usize,
    class: CompactionSourceClass,
    text: &str,
    origin: Option<CompactionSourceOrigin>,
) -> CompactionSourcePart {
    CompactionSourcePart {
        item_index,
        part_index: 0,
        class,
        text: text.to_string(),
        origin,
    }
}

fn published(mut envelope: ResponseItemEnvelope) -> ResponseItemEnvelope {
    if let Some(metadata) = &mut envelope.metadata {
        metadata.assistant_delivery_classification = Some(AssistantDeliveryClassification::Final);
    }
    envelope
}

fn at_part(mut part: CompactionSourcePart, part_index: usize) -> CompactionSourcePart {
    part.part_index = part_index;
    part
}

#[test]
fn snapshot_maps_sent_parts_to_recorded_originals_and_fails_closed() {
    let thread_id = ThreadId::new();
    // Dropped by the request builder, so later wire positions shift down by one.
    let configuration = ResponseItemEnvelope::new(ResponseItem::ConfigurationUpdate {
        reasoning: codex_protocol::models::ConfigurationReasoning {
            effort: codex_protocol::openai_models::ReasoningEffort::High,
        },
    });
    let user = sourced(
        message("user", &["use sqlite", "second part"]),
        RetainedSourceRole::User,
        "r1",
        1,
    );
    let assistant = published(sourced(
        message("assistant", &["done"]),
        RetainedSourceRole::Assistant,
        "r2",
        2,
    ));
    // Recorded original differs from the sent bytes: mapping exists but is not identical.
    let changed_recorded = sourced(
        message("user", &["full text"]),
        RetainedSourceRole::User,
        "r3",
        3,
    );
    let mut changed_sent = changed_recorded.clone();
    changed_sent.item = message("user", &["full"]);
    // A user-role item claiming an assistant source is a spoofed speaker.
    let spoofed = sourced(
        message("user", &["approved"]),
        RetainedSourceRole::Assistant,
        "r4",
        4,
    );
    // Delivery never proved these recorded bytes were what the user saw.
    let unpublished = sourced(
        message("assistant", &["hidden"]),
        RetainedSourceRole::Assistant,
        "r5",
        5,
    );
    let summary = ResponseItemEnvelope::new(message(
        "user",
        &[&format!("{}\nold summary", crate::compact::SUMMARY_PREFIX)],
    ));
    let prompt = ResponseItemEnvelope::new(message("user", &["summarize"]));
    let sent = vec![
        configuration,
        user,
        assistant,
        changed_sent,
        spoofed,
        unpublished,
        summary,
        prompt,
    ];
    let mut recorded = sent.clone();
    recorded[3] = changed_recorded;

    let filtered = snapshot_request(
        thread_id,
        "turn-9",
        2,
        &recorded,
        &sent,
        ConfigurationUpdates::Filtered,
    );
    let sent_updates = snapshot_request(
        thread_id,
        "turn-9",
        2,
        &recorded,
        &sent,
        ConfigurationUpdates::Sent,
    );

    let expected = |shift: usize| CompactionRequestSnapshot {
        thread_id,
        turn_id: "turn-9".to_string(),
        attempt: 2,
        sequence_watermark: Some(5),
        parts: vec![
            part(
                shift,
                CompactionSourceClass::User,
                "use sqlite",
                origin("r1", 1, true),
            ),
            at_part(
                part(
                    shift,
                    CompactionSourceClass::User,
                    "second part",
                    origin("r1", 1, true),
                ),
                1,
            ),
            part(
                shift + 1,
                CompactionSourceClass::Assistant,
                "done",
                origin("r2", 2, true),
            ),
            part(
                shift + 2,
                CompactionSourceClass::User,
                "full",
                origin("r3", 3, false),
            ),
            part(shift + 3, CompactionSourceClass::User, "approved", None),
            part(shift + 4, CompactionSourceClass::Assistant, "hidden", None),
            part(
                shift + 5,
                CompactionSourceClass::CompactionOutput,
                &format!("{}\nold summary", crate::compact::SUMMARY_PREFIX),
                None,
            ),
            part(
                shift + 6,
                CompactionSourceClass::HostContext,
                "summarize",
                None,
            ),
        ],
    };
    assert_eq!((filtered, sent_updates), (expected(0), expected(1)));
}

#[test]
fn separates_one_terminal_trailer_from_the_summary() {
    let trailer = format!("{OPENING}\n{{\"version\":1}}\n</capsule_sidecar_v1>");
    let mut output = vec![message(
        "assistant",
        &[&format!("Summary prose.\n\n{trailer}")],
    )];

    let separated = separate_trailer(&mut output, OPENING).expect("separable");

    assert_eq!(
        (separated, output),
        (
            CompactionTrailer::Present(trailer),
            vec![message("assistant", &["Summary prose."])]
        )
    );
}

#[test]
fn any_opening_occurrence_starts_the_trailer_even_across_parts_or_malformed() {
    let (head, tail) = OPENING.split_at(10);
    let split = vec![message(
        "assistant",
        &[
            "Summary A.",
            &format!(" More.\n{head}"),
            &format!("{tail}\n{{}}"),
            "rest",
        ],
    )];
    let malformed = vec![message(
        "assistant",
        &[&format!("Summary B.\n{OPENING}{{}}")],
    )];
    let inline = vec![message(
        "assistant",
        &[&format!("Summary C quotes `{OPENING}` once.")],
    )];

    let results = [split, malformed, inline].map(|mut output| {
        let separated = separate_trailer(&mut output, OPENING).expect("separable");
        (separated, output)
    });

    assert_eq!(
        results,
        [
            (
                CompactionTrailer::Present(format!("{OPENING}\n{{}}rest")),
                vec![message("assistant", &["Summary A.", " More."])],
            ),
            (
                CompactionTrailer::Present(format!("{OPENING}{{}}")),
                vec![message("assistant", &["Summary B."])],
            ),
            (
                CompactionTrailer::Present(format!("{OPENING}` once.")),
                vec![message("assistant", &["Summary C quotes `"])],
            ),
        ]
    );
}

#[test]
fn ambiguous_or_summaryless_output_fails_the_attempt() {
    let repeated = format!("Summary\n{OPENING}\n{{}}\n{OPENING}\n{{}}");
    let summaryless = format!("{OPENING}\n{{}}");
    let hidden_only = format!("<oai-mem-citation>doc</oai-mem-citation>{OPENING}\n{{}}");
    let earlier_message = vec![
        message("assistant", &[&format!("Summary\n{OPENING}\n{{}}")]),
        message("assistant", &["later"]),
    ];
    for mut output in [
        vec![message("assistant", &[&repeated])],
        vec![message("assistant", &[&summaryless])],
        vec![message("assistant", &[&hidden_only])],
        earlier_message,
    ] {
        assert!(separate_trailer(&mut output, OPENING).is_err());
    }
}

#[test]
fn oversize_trailer_is_discarded() {
    let body = "x".repeat(MAX_COMPACTION_EXTRACTION_TRAILER_BYTES);
    let mut output = vec![message(
        "assistant",
        &[&format!("Summary\n{OPENING}\n{body}")],
    )];

    let separated = separate_trailer(&mut output, OPENING).expect("separable");

    assert_eq!(
        (separated, output),
        (
            CompactionTrailer::Oversize,
            vec![message("assistant", &["Summary"])]
        )
    );
}

#[test]
fn opening_may_appear_in_the_instruction_but_not_in_sources() {
    let preparation = CompactionExtractionPreparation {
        instruction: format!("End with:\n{OPENING}\n{{\"version\":1}}\n</capsule_sidecar_v1>"),
        trailer_opening: OPENING.to_string(),
    };
    let request = |text: &str| CompactionRequestSnapshot {
        thread_id: ThreadId::new(),
        turn_id: "turn-1".to_string(),
        attempt: 1,
        sequence_watermark: None,
        parts: vec![part(0, CompactionSourceClass::User, text, None)],
    };

    assert_eq!(
        (
            admissible(&preparation, &request("plain")),
            admissible(&preparation, &request(&format!("quoted {OPENING}"))),
        ),
        (true, false)
    );
}
