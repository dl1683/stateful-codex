use pretty_assertions::assert_eq;

use super::sentences;
use super::uncovered_sentences;
use crate::AcceptanceCriterion;
use crate::AcceptanceKind;
use crate::AcceptanceLedger;
use crate::AcceptanceOrigin;
use crate::AcceptanceState;
use crate::RequestSpan;
use crate::StatefulRunId;

fn ledger() -> AcceptanceLedger {
    AcceptanceLedger::empty(StatefulRunId::parse("run").expect("run id"))
}

fn criterion(
    ordinal: u32,
    origin: AcceptanceOrigin,
    state: AcceptanceState,
    required: bool,
    goal: &str,
    quote: &str,
) -> AcceptanceCriterion {
    let start = goal.find(quote).expect("quote in goal");
    AcceptanceCriterion {
        id: format!("run#C{ordinal}"),
        ordinal,
        origin,
        kind: AcceptanceKind::Check,
        state,
        statement: quote.to_string(),
        requirement: quote.to_string(),
        required,
        depends_on: Vec::new(),
        milestone: None,
        request_span: Some(RequestSpan {
            start,
            end: start + quote.len(),
        }),
        artifacts: Vec::new(),
        checker: Vec::new(),
        check_command: None,
        check_cwd: None,
        expected_observation: None,
        plan: None,
        dismissal: None,
        note: None,
        revision: 1,
        ledger_revision: 1,
        evidence: None,
    }
}

fn quotes(goal: &str, spans: Vec<RequestSpan>) -> Vec<&str> {
    spans
        .into_iter()
        .map(|span| span.quote(goal).expect("span"))
        .collect()
}

#[test]
fn a_partial_clause_leaves_the_whole_sentence_uncovered() {
    let goal = "Write report.txt and values.csv with reconciled totals.";
    let mut ledger = ledger();
    ledger.criteria.push(criterion(
        1,
        AcceptanceOrigin::User,
        AcceptanceState::Active,
        true,
        goal,
        "Write report.txt",
    ));
    assert_eq!(quotes(goal, uncovered_sentences(goal, &ledger)), vec![goal]);
}

#[test]
fn derived_retired_and_optional_overlaps_do_not_cover_user_text() {
    let goal = "Fix the parser. Keep the API stable.";
    let mut ledger = ledger();
    ledger.criteria.push(criterion(
        1,
        AcceptanceOrigin::Derived,
        AcceptanceState::Active,
        true,
        goal,
        "Fix the parser.",
    ));
    ledger.criteria.push(criterion(
        2,
        AcceptanceOrigin::Derived,
        AcceptanceState::Retired,
        true,
        goal,
        "Keep the API stable.",
    ));
    assert_eq!(
        quotes(goal, uncovered_sentences(goal, &ledger)),
        vec!["Fix the parser.", "Keep the API stable."]
    );
    ledger.criteria.push(criterion(
        3,
        AcceptanceOrigin::User,
        AcceptanceState::Active,
        true,
        goal,
        "Fix the parser.",
    ));
    assert_eq!(
        quotes(goal, uncovered_sentences(goal, &ledger)),
        vec!["Keep the API stable."]
    );
}

#[test]
fn only_a_run_observed_without_any_action_owes_no_coverage() {
    // The no-tool exemption rests on host-recorded facts, never on the request's length.
    let goal = "Compute the totals for a, b and c and write a summary.";
    let mut observed = ledger();
    observed.observed_by_this_process = true;
    observed.completion_attempts = 1;
    assert_eq!(uncovered_sentences(goal, &observed), Vec::new());
    let mut acted = observed.clone();
    acted.host_actions = 1;
    assert_eq!(quotes(goal, uncovered_sentences(goal, &acted)), vec![goal]);
    let mut retried = observed;
    retried.completion_attempts = 2;
    assert_eq!(
        quotes(goal, uncovered_sentences(goal, &retried)),
        vec![goal]
    );
    // A run this process did not observe from its start owes coverage however short.
    assert_eq!(
        quotes(goal, uncovered_sentences(goal, &ledger())),
        vec![goal]
    );
}

#[test]
fn sentences_keep_their_punctuation_and_drop_separators() {
    let goal =
        "Fix src/parse.py. Write out/report.json.\nAll tests must pass; do not modify tests/.";
    assert_eq!(
        quotes(goal, sentences(goal)),
        vec![
            "Fix src/parse.py.",
            "Write out/report.json.",
            "All tests must pass",
            "do not modify tests/."
        ]
    );
}
