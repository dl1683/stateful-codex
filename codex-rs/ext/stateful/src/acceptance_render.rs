//! Bounded, model-facing views of the acceptance ledger: the run packet block, the effort
//! line, and the completion basis appended to a completed run's durable result.

use std::collections::BTreeMap;

use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::CriterionVerdict;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::ledger_verdicts;

use crate::acceptance_policy::RiskProfile;
use crate::acceptance_policy::VerificationReserve;

/// Longest statement shown per criterion line; the exact text is in the paged read.
const MAX_RENDERED_STATEMENT_BYTES: usize = 200;
const ELAPSED_STEP_SECONDS: u64 = 15 * 60;

/// Everything the run packet needs to show acceptance state and remaining capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AcceptanceView {
    pub(crate) ledger: AcceptanceLedger,
    pub(crate) risk: RiskProfile,
    pub(crate) reserve: VerificationReserve,
    /// Elapsed run time, rounded down to 15-minute steps so the packet does not churn.
    pub(crate) elapsed_seconds: u64,
}

impl AcceptanceView {
    pub(crate) fn new(run: &StatefulRun, ledger: AcceptanceLedger, now_ms: i64) -> Self {
        let risk = RiskProfile::assess(&ledger);
        let elapsed =
            u64::try_from(now_ms.saturating_sub(run.created_at_ms) / 1_000).unwrap_or_default();
        Self {
            reserve: VerificationReserve::for_risk(risk, run.value.budget),
            ledger,
            risk,
            elapsed_seconds: elapsed - elapsed % ELAPSED_STEP_SECONDS,
        }
    }

    /// The ledger lines of the run packet, headed by one summary line.
    pub(crate) fn ledger_lines(&self) -> Vec<String> {
        let ledger = &self.ledger;
        if ledger.criteria.is_empty() {
            let line = format!(
                "Acceptance ledger: empty (revision {}). When work changes files or has several requirements, record each early with stateful_acceptance_update; a one-sentence lookup needs none.",
                ledger.revision
            );
            return vec![line];
        }
        // The packet judges with the gate's own policy, assuming the files are unchanged since
        // their evidence; completion re-reads them.
        let verdicts = projected_verdicts(ledger);
        let current = verdicts
            .iter()
            .filter(|(_, verdict)| !verdict.is_unmet())
            .count();
        let mut lines = vec![format!(
            "Acceptance ledger (revision {}; pass expectedLedgerRevision: {} to stateful_acceptance_update): {current} of {} criteria settled. Completion needs every request sentence (goal and applied steering) covered, every proposal reviewed, applied steering reconciled, and every required criterion satisfied by a current receipt of its host-admitted check plan; otherwise set the run blocked with a partial result.",
            ledger.revision,
            ledger.revision,
            ledger.criteria.len()
        )];
        lines.extend(verdicts.iter().filter_map(|(ordinal, verdict)| {
            Some(criterion_line(ledger.criterion(*ordinal)?, verdict))
        }));
        lines
    }

    /// Remaining Autonomous capacity and the reserved verification allowance.
    pub(crate) fn capacity_line(&self, run: &StatefulRun) -> String {
        let budget = run.value.budget;
        let remaining_continuations = budget
            .max_continuations
            .saturating_sub(run.continuations_used);
        let limit_minutes = u64::from(budget.max_elapsed_seconds) / 60;
        let remaining_minutes =
            u64::from(budget.max_elapsed_seconds).saturating_sub(self.elapsed_seconds) / 60;
        let mut line = format!(
            "{} of {} continuations used, {remaining_continuations} remain; elapsed limit {limit_minutes} min, about {remaining_minutes} min remain.",
            run.continuations_used, budget.max_continuations
        );
        if self.reserve == VerificationReserve::default() {
            return line;
        }
        let reserve_minutes = u64::from(self.reserve.seconds) / 60;
        let reached = remaining_continuations <= self.reserve.continuations
            || remaining_minutes <= reserve_minutes;
        line.push_str(&format!(
            " Verification reserve ({}): {} continuations / {reserve_minutes} min.",
            self.risk.labels().join(", "),
            self.reserve.continuations
        ));
        line.push_str(if reached {
            " Reserve reached: stop new scope; verify and repair the declared criteria, or report a partial result with the evidence."
        } else {
            " When remaining capacity reaches it, stop new scope and verify or repair the declared criteria; if capacity cannot cover a planned check, say so before claiming success."
        });
        line
    }
}

/// Verdicts by the gate's own policy, assuming each criterion's files are as its evidence
/// and plan last saw them (the gate re-reads them at completion).
fn projected_verdicts(ledger: &AcceptanceLedger) -> Vec<(u32, CriterionVerdict)> {
    let observed = |digest: String| ArtifactState::Observed {
        digest,
        missing: Vec::new(),
    };
    let artifacts = ledger
        .criteria
        .iter()
        .filter_map(|criterion| {
            let digest = criterion.evidence.as_ref()?.artifact_digest.clone()?;
            Some((criterion.ordinal, observed(digest)))
        })
        .collect::<BTreeMap<_, _>>();
    let checkers = ledger
        .criteria
        .iter()
        .filter_map(|criterion| {
            let digest = criterion.plan.as_ref()?.checker_digest.clone();
            Some((criterion.ordinal, observed(digest)))
        })
        .collect::<BTreeMap<_, _>>();
    ledger_verdicts(ledger, &artifacts, &checkers)
}

fn criterion_line(criterion: &AcceptanceCriterion, verdict: &CriterionVerdict) -> String {
    let status = match verdict {
        CriterionVerdict::SatisfiedByHost => format!(
            "passed `{}` per its host-admitted plan (files re-checked at completion)",
            criterion.check_command.as_deref().unwrap_or_default()
        ),
        CriterionVerdict::ArtifactsPresent => {
            "derived existence predicate (files re-checked at completion)".to_string()
        }
        CriterionVerdict::ManualObservation => {
            "manual observation pinned to its files (agent-written)".to_string()
        }
        CriterionVerdict::DisclosedUnverified(reason) => format!(
            "optional, unverified (disclosed): {}",
            single_line(&bounded(reason, 160))
        ),
        CriterionVerdict::Closed(reason) => {
            format!("closed: {}", single_line(&bounded(reason, 160)))
        }
        CriterionVerdict::Unmet(reason) => format!("UNMET: {}", single_line(&bounded(reason, 240))),
    };
    let artifacts = if criterion.artifacts.is_empty() {
        String::new()
    } else {
        format!(" artifacts: {}.", criterion.artifacts.join(", "))
    };
    let checker = if criterion.checker.is_empty() {
        String::new()
    } else {
        format!(" checker: {}.", criterion.checker.join(", "))
    };
    let mut labels = vec![
        origin_name(criterion.origin).to_string(),
        kind_name(criterion).to_string(),
        if criterion.required {
            "required"
        } else {
            "optional"
        }
        .to_string(),
    ];
    if criterion.plan.is_some() {
        labels.push("plan admitted".to_string());
    }
    if !criterion.depends_on.is_empty() {
        let dependencies = criterion
            .depends_on
            .iter()
            .map(|ordinal| format!("C{ordinal}"))
            .collect::<Vec<_>>();
        labels.push(format!("after {}", dependencies.join(", ")));
    }
    if let Some(milestone) = &criterion.milestone {
        labels.push(format!("before: {}", single_line(&bounded(milestone, 80))));
    }
    format!(
        "- {} [{}] {}{artifacts}{checker} -> {status}",
        criterion.alias(),
        labels.join("; "),
        single_line(&bounded(&criterion.statement, MAX_RENDERED_STATEMENT_BYTES)),
    )
}

/// The disclosed completion basis: every criterion's verdict, exactly as the gate judged it.
pub(crate) fn completion_basis(
    ledger: &AcceptanceLedger,
    verdicts: &[(u32, CriterionVerdict)],
) -> Vec<String> {
    verdicts
        .iter()
        .filter_map(|(ordinal, verdict)| {
            let criterion = ledger.criterion(*ordinal)?;
            let label = match verdict {
                CriterionVerdict::SatisfiedByHost => format!(
                    "agent-written check `{}` ran its host-admitted plan (checker {} frozen at admission) and exited 0 on the host against the pinned artifacts (expected: {})",
                    criterion.check_command.as_deref().unwrap_or_default(),
                    criterion.checker.join(", "),
                    single_line(&bounded(
                        criterion.expected_observation.as_deref().unwrap_or_default(),
                        160
                    ))
                ),
                CriterionVerdict::ArtifactsPresent => {
                    "derived existence predicate: the declared artifacts exist (content not checked)".to_string()
                }
                CriterionVerdict::ManualObservation => format!(
                    "manual observation (agent-written, not host-verified): {}",
                    single_line(&bounded(
                        criterion
                            .evidence
                            .as_ref()
                            .and_then(|evidence| evidence.detail.as_deref())
                            .unwrap_or_default(),
                        240
                    ))
                ),
                CriterionVerdict::DisclosedUnverified(reason) => format!(
                    "OPTIONAL, UNVERIFIED: {}",
                    single_line(&bounded(reason, 240))
                ),
                CriterionVerdict::Closed(reason) => format!(
                    "{}: {}",
                    if criterion.state == AcceptanceState::Dismissed {
                        "omission proposal dismissed"
                    } else {
                        "retired derived criterion"
                    },
                    single_line(&bounded(reason, 240))
                ),
                CriterionVerdict::Unmet(reason) => format!("UNMET: {reason}"),
            };
            Some(format!(
                "{} [{}] {}: {label}",
                criterion.alias(),
                origin_name(criterion.origin),
                single_line(&bounded(&criterion.statement, MAX_RENDERED_STATEMENT_BYTES)),
            ))
        })
        .chain(ledger.reconciled_steering.iter().map(|reconciled| {
            format!(
                "steering {} reconciled by the agent (agent-written, not host-verified): {}",
                reconciled.steering_id,
                single_line(&bounded(&reconciled.reason, 240))
            )
        }))
        .collect()
}

fn origin_name(origin: AcceptanceOrigin) -> &'static str {
    match origin {
        AcceptanceOrigin::User => "user",
        AcceptanceOrigin::Derived => "derived",
        AcceptanceOrigin::Omission => "omission",
    }
}

fn kind_name(criterion: &AcceptanceCriterion) -> &'static str {
    match criterion.kind {
        codex_stateful_runtime::AcceptanceKind::Deliverable => "deliverable",
        codex_stateful_runtime::AcceptanceKind::Constraint => "constraint",
        codex_stateful_runtime::AcceptanceKind::Check => "check",
        codex_stateful_runtime::AcceptanceKind::Manual => "manual",
        codex_stateful_runtime::AcceptanceKind::Existence => "existence",
    }
}

pub(crate) fn bounded(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_string();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "acceptance_render_tests.rs"]
mod tests;
