use codex_extension_api::PreviousWorldStateSection;
use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceEvidence;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::EvidenceOutcome;
use codex_stateful_runtime::EvidenceSource;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;
use pretty_assertions::assert_eq;

use super::AcceptanceView;
use crate::run_world_state::RunWorldStateStatus;
use crate::run_world_state::run_world_state_section;

const GOAL: &str = "All tests must pass and out/a.csv and out/b.csv must exist.";

fn run(continuations_used: u32) -> StatefulRun {
    StatefulRun {
        id: StatefulRunId::parse("run-1").expect("run id"),
        value: NewStatefulRun {
            project_id: "project-1".to_string(),
            thread_ids: vec!["thread-1".to_string()],
            goal: GOAL.to_string(),
            mode: WorkflowMode::Autonomous,
            budget: RunBudget {
                max_continuations: 24,
                max_elapsed_seconds: 14_400,
            },
        },
        status: StatefulRunStatus::Running,
        strategy: None,
        strategy_revision: 0,
        result: None,
        continuations_used,
        revision: 3,
        created_at_ms: 0,
        updated_at_ms: 1,
    }
}

fn criterion(ordinal: u32, statement: &str, passed_at: Option<u64>) -> AcceptanceCriterion {
    AcceptanceCriterion {
        id: format!("run-1#C{ordinal}"),
        ordinal,
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Check,
        state: AcceptanceState::Active,
        statement: statement.to_string(),
        requirement: statement.to_string(),
        required: true,
        depends_on: Vec::new(),
        milestone: None,
        request_span: None,
        artifacts: vec!["out.txt".to_string()],
        checker: vec!["verify.sh".to_string()],
        check_command: Some(format!("check-{ordinal}")),
        check_cwd: None,
        expected_observation: Some("exit 0".to_string()),
        plan: Some(codex_stateful_runtime::PlanAdmission {
            criterion_revision: 1,
            checker_digest: "sha256:checker".to_string(),
            ledger_revision: 1,
        }),
        dismissal: None,
        note: None,
        revision: 1,
        ledger_revision: 1,
        evidence: passed_at.map(|generation| AcceptanceEvidence {
            sequence: u64::from(ordinal),
            source: EvidenceSource::HostCommand,
            outcome: EvidenceOutcome::Passed,
            command: Some(format!("check-{ordinal}")),
            exit_code: Some(0),
            output_tail: None,
            output_digest: None,
            artifact_digest: Some("sha256:out".to_string()),
            checker_digest: Some("sha256:checker".to_string()),
            detail: None,
            workspace_generation: generation,
            criterion_revision: 1,
            source_id: "call".to_string(),
            observed_at_ms: 1,
        }),
    }
}

fn ledger(revision: u64, generation: u64, criteria: Vec<AcceptanceCriterion>) -> AcceptanceLedger {
    AcceptanceLedger {
        run_id: StatefulRunId::parse("run-1").expect("run id"),
        revision,
        workspace_generation: generation,
        observed_executions: 0,
        pending_commands: 0,
        reconciled_steering: Vec::new(),
        stalled_completions: 0,
        verification_attempt: 0,
        verification_lease_expires_at_ms: None,
        criteria,
    }
}

fn status(run: StatefulRun, ledger: AcceptanceLedger, elapsed_ms: i64) -> RunWorldStateStatus {
    RunWorldStateStatus::Available {
        acceptance: Some(Box::new(AcceptanceView::new(&run, ledger, elapsed_ms))),
        run: Box::new(run),
        obligation: None,
        steering: Vec::new(),
        steering_complete: true,
        checkpoint_due: None,
    }
}

#[test]
fn capacity_shows_remaining_budget_and_a_risk_reserve() {
    let two_required = vec![
        criterion(1, "Tests pass.", None),
        criterion(2, "Lint passes.", None),
    ];
    let view = AcceptanceView::new(&run(3), ledger(1, 0, two_required), 20 * 60 * 1_000);
    assert_eq!(
        view.capacity_line(&run(3)),
        "3 of 24 continuations used, 21 remain; elapsed limit 240 min, about 225 min remain. Verification reserve (all-or-nothing): 4 continuations / 48 min. When remaining capacity reaches it, stop new scope and verify or repair the declared criteria; if capacity cannot cover a planned check, say so before claiming success."
    );
    assert!(
        view.capacity_line(&run(20))
            .contains("Reserve reached: stop new scope")
    );
    let quick = AcceptanceView::new(&run(3), ledger(0, 0, Vec::new()), 0);
    assert_eq!(
        quick.capacity_line(&run(3)),
        "3 of 24 continuations used, 21 remain; elapsed limit 240 min, about 240 min remain."
    );
}

#[test]
fn stale_evidence_replaces_only_the_acceptance_block() {
    let criteria = vec![criterion(1, "All tests pass.", Some(0))];
    let previous = run_world_state_section(status(run(3), ledger(2, 0, criteria.clone()), 0));
    let current = run_world_state_section(status(run(3), ledger(2, 1, criteria), 0));
    let full = previous
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("full packet");
    assert!(
        full.body()
            .contains("- C1 [derived; check; required; plan admitted] All tests pass. artifacts: out.txt. checker: verify.sh. -> passed `check-1` per its host-admitted plan (files re-checked at completion)")
    );
    let delta = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("stale evidence renders");
    let body = delta.body();
    assert!(body.contains("Acceptance (replaces the previous acceptance ledger view):"));
    assert!(body.contains("- C1 [derived; check; required; plan admitted] All tests pass. artifacts: out.txt. checker: verify.sh. -> UNMET: stale: the workspace changed after `check-1` started; run it again"));
    assert!(!body.contains("Goal:"));
}

#[test]
fn a_full_ledger_stays_bounded_and_names_the_exact_read() {
    let criteria = (1..=32)
        .map(|ordinal| criterion(ordinal, &"Long requirement text. ".repeat(12), None))
        .collect();
    let rendered = run_world_state_section(status(run(0), ledger(5, 0, criteria), 0))
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("full packet");
    let (start, end) = rendered.markers();
    assert!(start.len() + rendered.body().len() + end.len() <= crate::limits::MAX_MODEL_ITEM_BYTES);
    assert!(rendered.body().contains("Acceptance shortened:"));
    assert!(rendered.body().contains("section=\"acceptance\""));
}

#[test]
fn the_settled_count_uses_the_gate_policy() {
    let observed = |origin: AcceptanceOrigin, digest: Option<&str>| {
        let mut manual = criterion(1, "The chart reads well.", None);
        manual.origin = origin;
        manual.kind = AcceptanceKind::Manual;
        manual.check_command = None;
        manual.plan = None;
        manual.evidence = Some(AcceptanceEvidence {
            sequence: 1,
            source: EvidenceSource::Manual,
            outcome: EvidenceOutcome::Observed,
            command: None,
            exit_code: None,
            output_tail: None,
            output_digest: None,
            artifact_digest: digest.map(str::to_string),
            checker_digest: None,
            detail: Some("Viewed it.".to_string()),
            workspace_generation: 0,
            criterion_revision: 1,
            source_id: "call".to_string(),
            observed_at_ms: 1,
        });
        manual
    };
    let header = |manual: AcceptanceCriterion| {
        AcceptanceView::new(&run(0), ledger(1, 0, vec![manual]), 0).ledger_lines()[0].clone()
    };
    // A user requirement is never settled by an observation, as the gate also says.
    assert!(
        header(observed(AcceptanceOrigin::User, Some("sha256:out")))
            .contains("0 of 1 criteria settled")
    );
    // A derived manual criterion is settled only when the observation is pinned to its files.
    assert!(header(observed(AcceptanceOrigin::Derived, None)).contains("0 of 1 criteria settled"));
    assert!(
        header(observed(AcceptanceOrigin::Derived, Some("sha256:out")))
            .contains("1 of 1 criteria settled")
    );
}
