use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use pretty_assertions::assert_eq;

use super::RiskProfile;
use super::VerificationReserve;

const BUDGET: RunBudget = RunBudget {
    max_continuations: 24,
    max_elapsed_seconds: 14_400,
};

fn criterion(ordinal: u32) -> AcceptanceCriterion {
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
        request_span: None,
        artifacts: Vec::new(),
        check_command: Some("pytest -q".to_string()),
        check_cwd: None,
        expected_observation: Some("all tests pass".to_string()),
        approved_by_steering: None,
        dismissal: None,
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

#[test]
fn an_empty_ledger_gets_no_reserve() {
    let empty = ledger(Vec::new());
    let risk = RiskProfile::assess(&empty);
    assert_eq!(risk, RiskProfile::default());
    assert_eq!(
        VerificationReserve::for_risk(risk, BUDGET),
        VerificationReserve::default()
    );
}

#[test]
fn declared_structure_sets_risk_and_a_proportional_reserve() {
    let mut first = criterion(1);
    first.artifacts = vec!["out/a.csv".to_string()];
    let mut second = criterion(2);
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
