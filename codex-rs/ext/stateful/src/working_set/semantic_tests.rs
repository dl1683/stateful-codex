use codex_extension_api::CompactionSourceClass;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::Exclusions;
use super::piggyback::Addition;
use super::piggyback::SpanResolution;
use super::piggyback::prepare_addition;
use super::piggyback::validate_result;
use super::piggyback_tests::ASSISTANT;
use super::piggyback_tests::LATER_USER;
use super::piggyback_tests::USER;
use super::piggyback_tests::quarter;
use super::piggyback_tests::result;
use super::piggyback_tests::snapshot;
use super::piggyback_tests::span;
use super::piggyback_tests::standard_request;
use super::piggyback_tests::trailer;
use super::semantic::DebtReason;
use super::semantic::ExtractionBatch;
use super::semantic::SemanticLedger;

fn batch(records: Value, supersessions: Value) -> (Addition, ExtractionBatch) {
    let request = standard_request();
    let addition = prepare_addition(&request, 1, Some(&quarter)).expect("fits");
    let records = substitute(&addition, records);
    let supersessions = substitute(&addition, supersessions);
    let extraction = result(
        &request,
        &addition,
        trailer(&addition, records, supersessions),
    );
    let (outcome, _) = validate_result(&extraction, &addition, SpanResolution::Strict);
    let batch =
        ExtractionBatch::from_outcome(outcome, &addition.request_sha256, addition.unindexed);
    (addition, batch)
}

/// Replaces `["q", needle]` pairs with exact spans of index entry `q`.
fn substitute(addition: &Addition, value: Value) -> Value {
    let request = standard_request();
    match value {
        Value::Array(items) if items.len() == 2 && items[0].is_u64() && items[1].is_string() => {
            let q = items[0].as_u64().unwrap_or_default() as usize;
            span(addition, &request, q, items[1].as_str().unwrap_or_default())
        }
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| substitute(addition, v)).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, substitute(addition, v)))
                .collect(),
        ),
        other => other,
    }
}

fn rendered(ledger: &SemanticLedger) -> Vec<String> {
    ledger
        .eligible(&Exclusions::default())
        .iter()
        .map(|record| record.render("@s1"))
        .collect()
}

#[test]
fn semantic_kinds_render_labels_uncertainty_reasons_and_runs() {
    let (_, batch) = batch(
        json!([
            {"kind": "measurement", "value": [1, "4,108.3 invocations/s"],
             "subject": [1, "Baseline"], "run": [1, "run 1"], "qualifier": null},
            {"kind": "diagnosis", "finding": [1, "2206.04615.md is truncated"],
             "subject": [1, "2206.04615.md"], "uncertainty": [1, "Likely explanation"]},
            {"kind": "diagnosis", "finding": [1, "2206.04615.md is truncated"],
             "subject": [1, "2206.04615.md"], "uncertainty": null},
            {"kind": "choice", "choice": [1, "I chose lazy conversion"],
             "reason": [1, "it keeps defaults cheap"], "subject": [0, "Keep the old parser"]},
            {"kind": "rejection", "rejected": [1, "tools/claims_index.py is wrong, so I will not use it"],
             "reason": [1, "it zips README ids with sorted files"], "subject": [1, "tools/claims_index.py"]},
            {"kind": "measurement", "value": [1, "4,108.3"],
             "subject": [1, "Baseline"], "run": null, "qualifier": null},
        ]),
        json!([]),
    );
    let mut ledger = SemanticLedger::default();
    ledger.merge(batch);
    assert_eq!(
        rendered(&ledger),
        vec![
            "rejected(reported): \"tools/claims_index.py is wrong, so I will not use it\" because \"it zips README ids with sorted files\" @s1".to_string(),
            "diagnosis(reported; \"Likely explanation\"): \"2206.04615.md is truncated\" @s1".to_string(),
            "diagnosis(reported; uncertainty unspecified): \"2206.04615.md is truncated\" @s1".to_string(),
            // A user-quoted subject in a reported record still renders `reported`.
            "choice(reported): \"I chose lazy conversion\" for \"Keep the old parser\" because \"it keeps defaults cheap\" @s1".to_string(),
            "measured(reported; run \"run 1\"): \"4,108.3 invocations/s\" of \"Baseline\" @s1".to_string(),
        ]
    );
    // The unitless value is incomplete and dropped whole; its unknown run is still debt.
    assert_eq!(ledger.debt.0.get(&DebtReason::InvalidSpan), Some(&1));
    assert_eq!(ledger.debt.0.get(&DebtReason::UnknownRun), Some(&1));
}

#[test]
fn user_records_are_user_statements_and_future_sources_never_enter() {
    let (_, batch) = batch(
        json!([
            {"kind": "choice", "choice": [0, "Use lazy conversion for defaults"],
             "reason": [0, "Keep the old parser"], "subject": [0, "defaults"]},
            // The later-discovered cause is beyond the watermark and is not even indexed.
            {"kind": "diagnosis", "finding": [2, "drop the old parser"],
             "subject": [2, "old parser"], "uncertainty": null},
        ]),
        json!([]),
    );
    assert_eq!(batch.records.len(), 2);
    let mut ledger = SemanticLedger::default();
    ledger.merge(batch);
    assert_eq!(
        rendered(&ledger)[1],
        "choice(user): \"Use lazy conversion for defaults\" because \"Keep the old parser\" @s1"
    );
    // A quote naming the future cause cannot validate in any indexed part.
    let request = standard_request();
    let addition = prepare_addition(&request, 1, Some(&quarter)).expect("fits");
    assert!(
        addition
            .positions
            .iter()
            .all(|&p| !request.parts[p].text.contains("ecd03b4"))
    );
}

#[test]
fn carry_forward_keeps_originals_coalesces_restatements_and_new_runs() {
    let (_, first) = batch(
        json!([{"kind": "measurement", "value": [1, "4,108.3 invocations/s"],
                "subject": [1, "Baseline"], "run": [1, "run 1"], "qualifier": null}]),
        json!([]),
    );
    let mut ledger = SemanticLedger::default();
    ledger.merge(first.clone());
    // The same record again (identity duplicate) coalesces.
    ledger.merge(first);
    // A malformed boundary adds nothing and keeps prior records eligible.
    ledger.merge(ExtractionBatch::from_outcome(
        super::source_validation::SidecarOutcome::Absent,
        "req-2",
        0,
    ));
    // A later run of the same subject is a new record, never a replacement of the baseline.
    let later = snapshot(&[
        (CompactionSourceClass::User, USER, Some(1)),
        (
            CompactionSourceClass::Assistant,
            "Current run 2: 2,989.6 invocations/s.",
            Some(6),
        ),
    ]);
    let addition = prepare_addition(&later, 5, Some(&quarter)).expect("fits");
    let records = json!([{
        "kind": "measurement",
        "value": span(&addition, &later, 1, "2,989.6 invocations/s"),
        "subject": span(&addition, &later, 1, "Current"),
        "run": span(&addition, &later, 1, "run 2"),
        "qualifier": null,
    }]);
    let (outcome, _) = validate_result(
        &result(&later, &addition, trailer(&addition, records, json!([]))),
        &addition,
        SpanResolution::Strict,
    );
    ledger.merge(ExtractionBatch::from_outcome(outcome, "req-3", 0));
    assert_eq!(
        rendered(&ledger),
        vec![
            "measured(reported; run \"run 1\"): \"4,108.3 invocations/s\" of \"Baseline\" @s1"
                .to_string(),
            "measured(reported; run \"run 2\"): \"2,989.6 invocations/s\" of \"Current\" @s1"
                .to_string(),
        ]
    );
    assert_eq!(ledger.unavailable_boundaries, 1);
    // Eligibility is rechecked at disclosure: a forgotten source turn withdraws its record.
    let exclusions = Exclusions {
        turns: vec!["turn-2".to_string()],
        retired_texts: Vec::new(),
    };
    assert_eq!(ledger.eligible(&exclusions).len(), 1);
    // Payload round trip keeps the original provenance.
    let payload = serde_json::to_string(&ledger).expect("serializes");
    assert_eq!(
        serde_json::from_str::<SemanticLedger>(&payload).expect("parses"),
        ledger
    );
    assert!(ASSISTANT.contains("4,108.3") && LATER_USER.contains("drop"));
}
