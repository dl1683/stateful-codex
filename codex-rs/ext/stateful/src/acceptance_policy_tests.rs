use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::RequestSpan;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use pretty_assertions::assert_eq;

use super::RiskProfile;
use super::VerificationReserve;
use super::is_substantial;
use super::omission_proposals;

const GOAL: &str = "Fix the parser in src/parse.py. Write the summary to out/report.json.\nAll tests must pass; do not modify tests/.";
const BUDGET: RunBudget = RunBudget {
    max_continuations: 24,
    max_elapsed_seconds: 14_400,
};

fn criterion(ordinal: u32, span: Option<RequestSpan>) -> AcceptanceCriterion {
    AcceptanceCriterion {
        id: format!("run#C{ordinal}"),
        ordinal,
        origin: AcceptanceOrigin::User,
        kind: AcceptanceKind::Check,
        state: AcceptanceState::Active,
        statement: "requirement".to_string(),
        requirement: "requirement".to_string(),
        required: true,
        depends_on: Vec::new(),
        milestone: None,
        request_span: span,
        artifacts: Vec::new(),
        check_command: Some("pytest -q".to_string()),
        expected_observation: Some("all tests pass".to_string()),
        note: None,
        revision: 1,
        ledger_revision: 1,
        evidence: None,
    }
}

fn ledger(criteria: Vec<AcceptanceCriterion>) -> AcceptanceLedger {
    let mut ledger = AcceptanceLedger::empty(StatefulRunId::parse("run").expect("run id"));
    ledger.criteria = criteria;
    ledger
}

fn ledger_with_user_span(quote: &str) -> AcceptanceLedger {
    let start = GOAL.find(quote).expect("quote in goal");
    ledger(vec![criterion(
        1,
        Some(RequestSpan {
            start,
            end: start + quote.len(),
        }),
    )])
}

#[test]
fn omission_check_proposes_the_deliberately_omitted_output_gate() {
    let ledger = ledger_with_user_span("All tests must pass");
    let (proposals, beyond_cap) = omission_proposals(GOAL, &ledger);
    let statements = proposals
        .iter()
        .map(|(statement, span)| {
            assert_eq!(span.quote(GOAL), Some(statement.as_str()));
            statement.as_str()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        statements,
        vec![
            "Fix the parser in src/parse.py.",
            "Write the summary to out/report.json.",
            "do not modify tests/.",
        ]
    );
    assert_eq!(beyond_cap, 0);
}

#[test]
fn omission_check_never_reproposes_or_rewrites_covered_user_text() {
    let ledger = ledger_with_user_span("Write the summary to out/report.json.");
    let before = ledger.clone();
    let (proposals, _) = omission_proposals(GOAL, &ledger);
    assert!(
        proposals
            .iter()
            .all(|(statement, _)| !statement.contains("out/report.json"))
    );
    // The check only reads the ledger; it has no way to edit or drop a criterion.
    assert_eq!(ledger, before);
}

#[test]
fn one_sentence_lookup_gets_no_omission_check_and_no_reserve() {
    let empty = ledger(Vec::new());
    assert!(!is_substantial("What does parse_header return?", &empty));
    assert!(is_substantial(GOAL, &empty));
    let risk = RiskProfile::assess(&empty);
    assert_eq!(risk, RiskProfile::default());
    assert_eq!(
        VerificationReserve::for_risk(risk, BUDGET),
        VerificationReserve::default()
    );
    let mut changed = empty;
    changed.workspace_generation = 1;
    assert!(is_substantial("What does parse_header return?", &changed));
}

#[test]
fn declared_structure_sets_risk_and_a_proportional_reserve() {
    let mut first = criterion(1, None);
    first.artifacts = vec!["out/a.csv".to_string()];
    let mut second = criterion(2, None);
    second.artifacts = vec!["out/b.csv".to_string()];
    second.milestone = Some("file the declaration".to_string());
    let risk = RiskProfile::assess(&ledger(vec![first, second]));
    assert_eq!(
        risk.labels(),
        vec!["multi-artifact", "all-or-nothing", "irreversible milestone"]
    );
    assert_eq!(
        VerificationReserve::for_risk(risk, BUDGET),
        VerificationReserve {
            continuations: 4,
            seconds: 2_880,
        }
    );
}

#[test]
fn omission_proposals_are_capped_and_count_the_rest() {
    let goal = (1..=11)
        .map(|index| format!("Write output file number {index} to out/{index}.txt."))
        .collect::<Vec<_>>()
        .join(" ");
    let (proposals, beyond_cap) = omission_proposals(&goal, &ledger(Vec::new()));
    assert_eq!((proposals.len(), beyond_cap), (8, 3));
}
