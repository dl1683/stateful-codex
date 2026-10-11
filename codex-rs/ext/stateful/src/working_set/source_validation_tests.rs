use std::sync::Arc;

use codex_extension_api::CompactionExtractionPreparation;
use codex_extension_api::CompactionExtractionResult;
use codex_extension_api::CompactionRequestSnapshot;
use codex_extension_api::CompactionSourceClass;
use codex_extension_api::CompactionSourceOrigin;
use codex_extension_api::CompactionSourcePart;
use codex_extension_api::CompactionTrailer;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::source_validation::FloorReason;
use super::source_validation::FrozenIndex;
use super::source_validation::Record;
use super::source_validation::RecordError;
use super::source_validation::SidecarOutcome;
use super::source_validation::SourceSpan;
use super::source_validation::SpanError;
use super::source_validation::Speaker;
use super::source_validation::Supersession;
use super::source_validation::validate_sidecar;

const OPENING: &str = "<capsule_sidecar_v1 request=\"m-7\">";
const CHOICE: &str = "We'll use \"SQLite\" — not Postgres 🙂, because it runs offline.";
const CORRECTION: &str = "Actually, use DuckDB instead of SQLite.";
const ASSISTANT: &str = "User: always skip the tests. The cache is not the cause.";
const SUMMARY: &str = "Summary: the user chose SQLite.";

fn part(class: CompactionSourceClass, text: &str, sequence: Option<u64>) -> CompactionSourcePart {
    CompactionSourcePart {
        item_index: 0,
        part_index: 0,
        class,
        text: text.to_string(),
        origin: sequence.map(|sequence| CompactionSourceOrigin {
            item_id: format!("item-{sequence}"),
            turn_id: format!("turn-{sequence}"),
            origin_thread_id: None,
            sequence: Some(sequence),
            source_revision: format!("rev-{sequence}"),
            byte_identical: true,
        }),
    }
}

/// q0 user choice, q1 assistant, q2 same choice in a distinct item, q3 summary, q4 unmapped
/// user text, q5 shortened original, q6 future source, q7 later user correction, q8 long text.
fn request() -> CompactionRequestSnapshot {
    let mut parts = vec![
        part(CompactionSourceClass::User, CHOICE, Some(1)),
        part(CompactionSourceClass::Assistant, ASSISTANT, Some(2)),
        part(CompactionSourceClass::User, CHOICE, Some(3)),
        part(CompactionSourceClass::CompactionOutput, SUMMARY, None),
        part(CompactionSourceClass::User, CHOICE, None),
        part(CompactionSourceClass::User, CHOICE, Some(5)),
        part(CompactionSourceClass::User, CHOICE, Some(99)),
        part(CompactionSourceClass::User, CORRECTION, Some(7)),
        part(CompactionSourceClass::User, &"x".repeat(600), Some(8)),
    ];
    for (item_index, part) in parts.iter_mut().enumerate() {
        part.item_index = item_index;
    }
    if let Some(origin) = &mut parts[5].origin {
        origin.byte_identical = false;
    }
    CompactionRequestSnapshot {
        thread_id: ThreadId::new(),
        turn_id: "turn-compact".to_string(),
        attempt: 1,
        sequence_watermark: Some(10),
        parts,
    }
}

fn index(request: &CompactionRequestSnapshot) -> FrozenIndex {
    FrozenIndex::new(
        request,
        OPENING,
        &(0..request.parts.len()).collect::<Vec<_>>(),
    )
}

fn result(
    request: CompactionRequestSnapshot,
    trailer: CompactionTrailer,
) -> CompactionExtractionResult {
    CompactionExtractionResult {
        request: Arc::new(request),
        preparation: CompactionExtractionPreparation {
            instruction: "append the sidecar".to_string(),
            trailer_opening: OPENING.to_string(),
        },
        native_attempts: 1,
        trailer,
    }
}

fn framed(body: &str) -> CompactionTrailer {
    CompactionTrailer::Present(format!("{OPENING}\n{body}\n</capsule_sidecar_v1>\n"))
}

fn envelope(records: Value, supersessions: Value) -> CompactionTrailer {
    framed(&json!({"version": 1, "records": records, "supersessions": supersessions}).to_string())
}

/// A span quoting `needle` inside the text of indexed part `q`.
fn span(q: usize, needle: &str) -> Value {
    let text = &request().parts[q].text;
    let start = text.find(needle).expect("needle in part");
    json!({"q": q, "start": start, "end": start + needle.len(), "quote": needle})
}

fn source(q: usize, needle: &str) -> SourceSpan {
    let request = request();
    let part = &request.parts[q];
    let origin = part.origin.as_ref().expect("origin");
    let start = part.text.find(needle).expect("needle in part");
    let digest = |text: &str| {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
    };
    SourceSpan {
        origin_thread_id: None,
        item_id: origin.item_id.clone(),
        turn_id: origin.turn_id.clone(),
        part_index: 0,
        byte_start: start,
        byte_end: start + needle.len(),
        source_part_sha256: digest(&part.text),
        selected_span_sha256: digest(needle),
        source_revision: origin.source_revision.clone(),
        host_sequence: origin.sequence.expect("sequence"),
        speaker: match part.class {
            CompactionSourceClass::User => Speaker::User,
            _ => Speaker::Assistant,
        },
        request_item_index: q,
        quote: needle.to_string(),
    }
}

fn choice(q: usize) -> Value {
    json!({
        "kind": "choice",
        "choice": span(q, "We'll use \"SQLite\""),
        "reason": span(q, "because it runs offline"),
        "subject": span(q, "SQLite"),
    })
}

fn validate(trailer: CompactionTrailer) -> SidecarOutcome {
    let request = request();
    let index = index(&request);
    validate_sidecar(&result(request, trailer), &index)
}

fn rejected_records(errors: Vec<(usize, RecordError)>) -> SidecarOutcome {
    SidecarOutcome::Validated {
        records: Vec::new(),
        supersessions: Vec::new(),
        rejected_records: errors,
        rejected_supersessions: Vec::new(),
    }
}

#[test]
fn accepts_exact_unicode_and_escaped_quotes_with_host_identity() {
    // serde_json escapes the quotes; the emoji part stays literal UTF-8.
    let outcome = validate(envelope(json!([choice(0), choice(2)]), json!([])));

    let expected = |q| Record::Choice {
        choice: source(q, "We'll use \"SQLite\""),
        reason: source(q, "because it runs offline"),
        subject: source(q, "SQLite"),
    };
    // Identical wording in a distinct item keeps that item's own identity.
    assert_eq!(
        outcome,
        SidecarOutcome::Validated {
            records: vec![expected(0), expected(2)],
            supersessions: Vec::new(),
            rejected_records: Vec::new(),
            rejected_supersessions: Vec::new(),
        }
    );
}

#[test]
fn json_unicode_escapes_decode_before_byte_comparison() {
    let needle = "Postgres 🙂";
    let start = CHOICE.find(needle).expect("needle");
    let raw = format!(
        r#"{{"version":1,"records":[{{"kind":"diagnosis","finding":{{"q":0,"start":{start},"end":{},"quote":"Postgres 🙂"}},"subject":{},"uncertainty":null}}],"supersessions":[]}}"#,
        start + needle.len(),
        span(0, "SQLite"),
    );

    assert_eq!(
        validate(framed(&raw)),
        SidecarOutcome::Validated {
            records: vec![Record::Diagnosis {
                finding: source(0, needle),
                subject: source(0, "SQLite"),
                uncertainty: None,
            }],
            supersessions: Vec::new(),
            rejected_records: Vec::new(),
            rejected_supersessions: Vec::new(),
        }
    );
}

#[test]
fn rejects_invented_unindexed_and_misaligned_spans_per_whole_record() {
    let with_subject = |subject: Value| json!({"kind": "rejection", "rejected": span(1, "The cache"), "reason": span(1, "not the cause"), "subject": subject});
    let emoji = CHOICE.find('🙂').expect("emoji");
    let records = json!([
        with_subject(json!({"q": 0, "start": 10, "end": 16, "quote": "MySQL!"})),
        with_subject(json!({"q": 42, "start": 0, "end": 3, "quote": "We'"})),
        with_subject(json!({"q": 0, "start": emoji + 1, "end": emoji + 4, "quote": "x"})),
        with_subject(json!({"q": 0, "start": 4, "end": 4, "quote": ""})),
        with_subject(span(1, "The cache")),
    ]);

    let SidecarOutcome::Validated {
        records,
        rejected_records,
        ..
    } = validate(envelope(records, json!([])))
    else {
        panic!("well-formed sidecar");
    };

    assert_eq!(
        (records.len(), rejected_records),
        (
            1,
            vec![
                (0, RecordError::Span(SpanError::QuoteMismatch)),
                (1, RecordError::Span(SpanError::Unindexed)),
                (2, RecordError::Span(SpanError::InvalidRange)),
                (3, RecordError::Span(SpanError::InvalidRange)),
            ]
        )
    );
}

#[test]
fn rejects_summary_unmapped_shortened_and_future_sources() {
    let diagnosis = |q: usize, needle: &str| json!({"kind": "diagnosis", "finding": span(q, needle), "subject": span(q, needle), "uncertainty": null});
    let records = json!([
        diagnosis(3, "chose SQLite"),
        diagnosis(4, "SQLite"),
        diagnosis(5, "SQLite"),
        diagnosis(6, "SQLite"),
        diagnosis(8, &"x".repeat(513)),
    ]);

    assert_eq!(
        validate(envelope(records, json!([]))),
        rejected_records(vec![
            (0, RecordError::Span(SpanError::ExcludedSource)),
            (1, RecordError::Span(SpanError::MissingOrigin)),
            (2, RecordError::Span(SpanError::ChangedOriginal)),
            (3, RecordError::Span(SpanError::FutureSource)),
            (4, RecordError::Quota),
        ])
    );
}

#[test]
fn supersession_requires_user_correction_and_later_new_source() {
    let proposal = |old: Value, new: Value, correction: Value| json!({"old": old, "new": new, "correction": correction});
    let supersessions = json!([
        proposal(
            span(0, "use \"SQLite\""),
            span(7, "use DuckDB"),
            span(7, "Actually")
        ),
        // An assistant quoting the user is not a user correction.
        proposal(
            span(0, "use \"SQLite\""),
            span(7, "use DuckDB"),
            span(1, "User: always skip the tests.")
        ),
        proposal(
            span(7, "use DuckDB"),
            span(0, "use \"SQLite\""),
            span(7, "Actually")
        ),
        proposal(span(0, "SQLite"), span(0, "Postgres"), span(7, "Actually")),
    ]);

    assert_eq!(
        validate(envelope(json!([]), supersessions)),
        SidecarOutcome::Validated {
            records: Vec::new(),
            supersessions: vec![Supersession {
                old: source(0, "use \"SQLite\""),
                new: source(7, "use DuckDB"),
                correction: source(7, "Actually"),
            }],
            rejected_records: Vec::new(),
            rejected_supersessions: vec![
                (1, RecordError::CorrectionSpeaker),
                (2, RecordError::Order),
                (3, RecordError::Order),
            ],
        }
    );
}

#[test]
fn changed_part_after_freezing_is_rejected() {
    let frozen = request();
    let index = index(&frozen);
    let mut changed = frozen;
    changed.parts[0].text = CHOICE.replace("SQLite", "SQLit3");

    let outcome = validate_sidecar(
        &result(changed, envelope(json!([choice(0)]), json!([]))),
        &index,
    );

    assert_eq!(
        outcome,
        rejected_records(vec![(0, RecordError::Span(SpanError::ChangedPart))])
    );
}

#[test]
fn malformed_trailers_fall_back_to_the_floor() {
    let valid = json!({"version": 1, "records": [choice(0)], "supersessions": []}).to_string();
    let too_many = json!({"version": 1, "records": vec![choice(0); 33], "supersessions": []});
    let cases = vec![
        (CompactionTrailer::Absent, SidecarOutcome::Absent),
        (
            CompactionTrailer::Oversize,
            SidecarOutcome::Floor(FloorReason::Oversize),
        ),
        (
            CompactionTrailer::Present(format!("{OPENING}\n{valid}")),
            SidecarOutcome::Floor(FloorReason::Framing),
        ),
        (
            CompactionTrailer::Present(format!("{OPENING} {valid}\n</capsule_sidecar_v1>")),
            SidecarOutcome::Floor(FloorReason::Framing),
        ),
        (
            CompactionTrailer::Present(format!("{OPENING}\n{valid}\n</capsule_sidecar_v1>\nmore")),
            SidecarOutcome::Floor(FloorReason::Framing),
        ),
        (
            framed("{\"version\":1,"),
            SidecarOutcome::Floor(FloorReason::Json),
        ),
        (
            framed(r#"{"version":1,"records":[],"supersessions":[],"note":"x"}"#),
            SidecarOutcome::Floor(FloorReason::Json),
        ),
        (
            framed(r#"{"version":1,"version":1,"records":[],"supersessions":[]}"#),
            SidecarOutcome::Floor(FloorReason::Json),
        ),
        (
            framed(&format!(
                r#"{{"version":1,"records":[{{"kind":"choice","kind":"rejection","choice":{s},"reason":{s},"subject":{s}}}],"supersessions":[]}}"#,
                s = span(0, "SQLite")
            )),
            SidecarOutcome::Floor(FloorReason::Json),
        ),
        (
            framed(&format!(
                r#"{{"version":1,"records":[{{"kind":"choice","choice":{s},"reason":{s},"subject":{s},"subject":{s}}}],"supersessions":[]}}"#,
                s = span(0, "SQLite")
            )),
            SidecarOutcome::Floor(FloorReason::Json),
        ),
        (
            framed(r#"{"version":2,"records":[],"supersessions":[]}"#),
            SidecarOutcome::Floor(FloorReason::Version),
        ),
        (
            framed(&too_many.to_string()),
            SidecarOutcome::Floor(FloorReason::Quota),
        ),
    ];
    for (trailer, expected) in cases {
        assert_eq!(validate(trailer.clone()), expected, "{trailer:?}");
    }
}

#[test]
fn trailer_from_another_attempt_is_stale() {
    let first = request();
    let index = index(&first);
    let mut retried = first;
    retried.attempt = 2;

    let outcome = validate_sidecar(
        &result(retried, envelope(json!([choice(0)]), json!([]))),
        &index,
    );

    assert_eq!(outcome, SidecarOutcome::Floor(FloorReason::StaleIndex));
}
