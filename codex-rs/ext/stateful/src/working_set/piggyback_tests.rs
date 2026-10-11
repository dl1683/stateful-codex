use std::sync::Arc;

use codex_extension_api::CompactionExtractionContributor;
use codex_extension_api::CompactionExtractionResult;
use codex_extension_api::CompactionRequestSnapshot;
use codex_extension_api::CompactionSourceClass;
use codex_extension_api::CompactionSourceOrigin;
use codex_extension_api::CompactionSourcePart;
use codex_extension_api::CompactionTrailer;
use codex_extension_api::ExtensionData;
use codex_extension_api::MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::piggyback::Addition;
use super::piggyback::MAX_ADDITION_TOKENS;
use super::piggyback::PiggybackExtractor;
use super::piggyback::PiggybackState;
use super::piggyback::SpanResolution;
use super::piggyback::prepare_addition;
use super::piggyback::validate_result;
use super::semantic::DebtReason;
use super::source_validation::SidecarOutcome;

pub(super) const USER: &str = "Keep the old parser. Use lazy conversion for defaults.";
pub(super) const ASSISTANT: &str = "Likely explanation: 2206.04615.md is truncated. I chose lazy conversion because it keeps defaults cheap. tools/claims_index.py is wrong, so I will not use it: it zips README ids with sorted files. Baseline run 1: 4,108.3 invocations/s.";
pub(super) const LATER_USER: &str =
    "Actually, drop the old parser and port everything to the new one.";

pub(super) fn quarter(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Parts as (class, text, host sequence); `None` means no durable original mapping.
pub(super) fn snapshot(
    parts: &[(CompactionSourceClass, &str, Option<u64>)],
) -> CompactionRequestSnapshot {
    CompactionRequestSnapshot {
        thread_id: ThreadId::new(),
        turn_id: "turn-compact".to_string(),
        attempt: 1,
        sequence_watermark: Some(10),
        parts: parts
            .iter()
            .enumerate()
            .map(
                |(item_index, (class, text, sequence))| CompactionSourcePart {
                    item_index,
                    part_index: 0,
                    class: *class,
                    text: text.to_string(),
                    origin: sequence.map(|sequence| CompactionSourceOrigin {
                        item_id: format!("item-{sequence}"),
                        turn_id: format!("turn-{sequence}"),
                        origin_thread_id: None,
                        sequence: Some(sequence),
                        source_revision: format!("rev-{sequence}"),
                        byte_identical: true,
                    }),
                },
            )
            .collect(),
    }
}

/// Host context, user, assistant, summary, command output, later user, future user.
pub(super) fn standard_request() -> CompactionRequestSnapshot {
    snapshot(&[
        (
            CompactionSourceClass::HostContext,
            "<environment_context/>",
            None,
        ),
        (CompactionSourceClass::User, USER, Some(1)),
        (CompactionSourceClass::Assistant, ASSISTANT, Some(2)),
        (
            CompactionSourceClass::CompactionOutput,
            "Summary: lazy conversion.",
            None,
        ),
        (
            CompactionSourceClass::CommandOutput,
            "throughput: 9.9 invocations/s",
            None,
        ),
        (CompactionSourceClass::User, LATER_USER, Some(4)),
        (
            CompactionSourceClass::User,
            "Cause: commit ecd03b4.",
            Some(99),
        ),
    ])
}

/// A span of `needle` in the text of index entry `q`, with exact offsets.
pub(super) fn span(
    addition: &Addition,
    request: &CompactionRequestSnapshot,
    q: usize,
    needle: &str,
) -> Value {
    let text = &request.parts[addition.positions[q]].text;
    let start = text.find(needle).expect("needle in part");
    json!({"q": q, "start": start, "end": start + needle.len(), "quote": needle})
}

pub(super) fn trailer(
    addition: &Addition,
    records: Value,
    supersessions: Value,
) -> CompactionTrailer {
    CompactionTrailer::Present(format!(
        "{}\n{}\n</capsule_sidecar_v1>\n",
        addition.preparation.trailer_opening,
        json!({"version": 1, "records": records, "supersessions": supersessions})
    ))
}

pub(super) fn result(
    request: &CompactionRequestSnapshot,
    addition: &Addition,
    trailer: CompactionTrailer,
) -> CompactionExtractionResult {
    CompactionExtractionResult {
        request: Arc::new(request.clone()),
        preparation: addition.preparation.clone(),
        native_attempts: 1,
        trailer,
    }
}

#[test]
fn addition_indexes_only_mappable_originals_users_first() {
    let request = standard_request();
    let addition = prepare_addition(&request, 7, Some(&quarter)).expect("fits");
    // The future source and every host, summary or command part are excluded; q follows
    // request order.
    assert_eq!(addition.positions, vec![1, 2, 5]);
    assert_eq!(addition.unindexed, 0);
    let instruction = &addition.preparation.instruction;
    assert!(instruction.starts_with("Produce the ordinary handoff summary requested above first"));
    assert!(instruction.contains(&addition.preparation.trailer_opening));
    assert!(instruction.ends_with(&format!(
        "\n0: 1.0 {}B u\n1: 2.0 {}B a\n2: 5.0 {}B u",
        USER.len(),
        ASSISTANT.len(),
        LATER_USER.len()
    )));
    assert!(instruction.len() <= MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES);
    assert_eq!(addition.tokens, quarter(instruction));
    // The marker is request-scoped and absent from every source part.
    let other = prepare_addition(&request, 8, Some(&quarter)).expect("fits");
    assert!(other.preparation.trailer_opening != addition.preparation.trailer_opening);
    assert!(
        request
            .parts
            .iter()
            .all(|part| !part.text.contains(&addition.preparation.trailer_opening))
    );
}

#[test]
fn index_stops_at_the_token_bound_and_records_unindexed_debt() {
    let texts: Vec<String> = (0..200).map(|n| format!("user statement {n}")).collect();
    let parts: Vec<_> = texts
        .iter()
        .enumerate()
        .map(|(n, text)| (CompactionSourceClass::User, text.as_str(), Some(n as u64)))
        .collect();
    let mut request = snapshot(&parts);
    request.sequence_watermark = Some(1_000);
    // A dense tokenizer (one token per three bytes) binds before the 4,096-byte bound.
    let dense = |text: &str| text.len().div_ceil(3);
    let addition = prepare_addition(&request, 1, Some(&dense)).expect("instruction fits");
    assert!(addition.tokens <= MAX_ADDITION_TOKENS);
    assert!(addition.preparation.instruction.len() <= MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES);
    assert_eq!(addition.positions.len() + addition.unindexed, 200);
    assert!(addition.unindexed > 0);
    // Current (newest) user parts are indexed first.
    assert_eq!(addition.positions.last(), Some(&199));
    // Without the continuing model's tokenizer a byte counts as a token: the instruction
    // cannot fit and extraction is disabled for the attempt.
    assert!(prepare_addition(&request, 1, None).is_err());
}

#[test]
fn unique_quote_resolution_locates_spans_without_changing_quotes() {
    let request = standard_request();
    let addition = prepare_addition(&request, 3, Some(&quarter)).expect("fits");
    let mut wrong = span(&addition, &request, 1, "I chose lazy conversion");
    wrong["start"] = json!(0);
    wrong["end"] = json!(23);
    let mut misplaced = span(&addition, &request, 1, "it keeps defaults cheap");
    misplaced["q"] = json!(0);
    let records = json!([{
        "kind": "choice",
        "choice": wrong,
        "reason": misplaced,
        "subject": span(&addition, &request, 1, "lazy conversion"),
    }]);
    let outcome_result = result(&request, &addition, trailer(&addition, records, json!([])));
    let (strict, moved) = validate_result(&outcome_result, &addition, SpanResolution::Strict);
    let SidecarOutcome::Validated { records, .. } = strict else {
        panic!("well formed");
    };
    assert_eq!((records.len(), moved), (0, 0));
    let (resolved, moved) =
        validate_result(&outcome_result, &addition, SpanResolution::UniqueQuote);
    let SidecarOutcome::Validated { records, .. } = resolved else {
        panic!("well formed");
    };
    assert_eq!((records.len(), moved), (1, 2));
    // A quote occurring in two indexed parts stays ambiguous and is rejected.
    let mut ambiguous = span(&addition, &request, 1, "lazy conversion");
    ambiguous["start"] = json!(1);
    let records = json!([{
        "kind": "diagnosis",
        "finding": ambiguous,
        "subject": span(&addition, &request, 1, "2206.04615.md"),
        "uncertainty": null,
    }]);
    let ambiguous_result = result(&request, &addition, trailer(&addition, records, json!([])));
    let (outcome, moved) =
        validate_result(&ambiguous_result, &addition, SpanResolution::UniqueQuote);
    let SidecarOutcome::Validated { records, .. } = outcome else {
        panic!("well formed");
    };
    assert_eq!((records.len(), moved), (0, 0));
}

#[test]
fn contributor_merges_valid_records_and_floors_malformed_trailers_without_retry() {
    let extractor = PiggybackExtractor::new(Some(Arc::new(quarter)), SpanResolution::Strict);
    let store = ExtensionData::new("thread");
    let request = standard_request();
    let preparation = extractor.prepare(&request, &store).expect("prepared");
    let addition = {
        let state = store
            .get::<std::sync::Mutex<PiggybackState>>()
            .expect("state");
        let state = state.lock().expect("lock");
        state.pending.clone().expect("pending")
    };
    assert_eq!(preparation, addition.preparation);
    assert!(
        preparation
            .trailer_opening
            .starts_with("<capsule_sidecar_v1 request=\"")
    );
    let records = json!([{
        "kind": "rejection",
        "rejected": span(&addition, &request, 1, "tools/claims_index.py is wrong, so I will not use it"),
        "reason": span(&addition, &request, 1, "it zips README ids with sorted files"),
        "subject": span(&addition, &request, 1, "tools/claims_index.py"),
    }]);
    extractor.complete(
        result(&request, &addition, trailer(&addition, records, json!([]))),
        &store,
    );
    // The next attempt's trailer is malformed: no new records, semantic coverage unavailable.
    let second = snapshot(&[(CompactionSourceClass::User, LATER_USER, Some(4))]);
    extractor.prepare(&second, &store).expect("prepared");
    let pending = {
        let state = store
            .get::<std::sync::Mutex<PiggybackState>>()
            .expect("state");
        let state = state.lock().expect("lock");
        state.pending.clone().expect("pending")
    };
    extractor.complete(
        result(
            &second,
            &pending,
            CompactionTrailer::Present(format!(
                "{}\n{{\"version\":",
                pending.preparation.trailer_opening
            )),
        ),
        &store,
    );
    let state = store
        .get::<std::sync::Mutex<PiggybackState>>()
        .expect("state");
    let state = state.lock().expect("lock");
    assert_eq!(state.ledger.records.len(), 1);
    assert_eq!(state.ledger.unavailable_boundaries, 1);
    assert_eq!(
        state.ledger.debt.0.get(&DebtReason::SemanticUnavailable),
        Some(&1)
    );
    assert_eq!(state.pending, None);
}
