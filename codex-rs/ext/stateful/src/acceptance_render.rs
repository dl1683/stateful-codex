//! Bounded, model-facing views of the acceptance ledger: the run packet block, the effort
//! line, and the completion basis appended to a completed run's durable result.

use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::CriterionVerdict;
use codex_stateful_runtime::EvidenceOutcome;
use codex_stateful_runtime::StatefulRun;

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
        let current = ledger
            .criteria
            .iter()
            .filter(|criterion| status_label(criterion, ledger.workspace_generation).1)
            .count();
        let mut lines = vec![format!(
            "Acceptance ledger (revision {}; pass expectedLedgerRevision: {} to stateful_acceptance_update): {current} of {} criteria settled. Completion needs every goal sentence covered, every proposal reviewed and every required criterion satisfied by a user-approved check pinned to artifacts; otherwise set the run blocked with a partial result.",
            ledger.revision,
            ledger.revision,
            ledger.criteria.len()
        )];
        lines.extend(
            ledger
                .criteria
                .iter()
                .map(|criterion| criterion_line(criterion, ledger.workspace_generation)),
        );
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

fn criterion_line(criterion: &AcceptanceCriterion, generation: u64) -> String {
    let (status, _) = status_label(criterion, generation);
    let artifacts = if criterion.artifacts.is_empty() {
        String::new()
    } else {
        format!(" artifacts: {}.", criterion.artifacts.join(", "))
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
    let expected = criterion
        .expected_observation
        .as_deref()
        .map(|expected| format!(" expects: {}.", single_line(&bounded(expected, 120))))
        .unwrap_or_default();
    format!(
        "- {} [{}] {}{artifacts}{expected} -> {status}",
        criterion.alias(),
        labels.join("; "),
        single_line(&bounded(&criterion.statement, MAX_RENDERED_STATEMENT_BYTES)),
    )
}

/// A packet status for a criterion, without reading artifacts, and whether it is settled.
fn status_label(criterion: &AcceptanceCriterion, generation: u64) -> (String, bool) {
    match criterion.state {
        AcceptanceState::Proposed => {
            return (
                "PROPOSED by the omission check: accept it, or dismiss it only with a user steering receipt or a covering user criterion".to_string(),
                false,
            );
        }
        AcceptanceState::Dismissed | AcceptanceState::Retired => {
            return (
                format!(
                    "{}: {}",
                    if criterion.state == AcceptanceState::Dismissed {
                        "dismissed"
                    } else {
                        "retired"
                    },
                    single_line(&bounded(criterion.note.as_deref().unwrap_or_default(), 160))
                ),
                true,
            );
        }
        AcceptanceState::Active => {}
    }
    let current = criterion
        .evidence
        .as_ref()
        .filter(|evidence| evidence.criterion_revision == criterion.revision);
    let check = criterion.check_command.as_deref();
    match (current, check) {
        (Some(evidence), _) => {
            let fresh = evidence.workspace_generation == generation;
            match (evidence.outcome, fresh) {
                (EvidenceOutcome::Passed, true) if criterion.approved_by_steering.is_some() => (
                    format!(
                        "user-approved check `{}` passed (host-observed exit 0)",
                        check.unwrap_or_default()
                    ),
                    true,
                ),
                (EvidenceOutcome::Passed, true) => (
                    format!(
                        "RECEIPT ONLY: `{}` passed, but the user has not approved it as this criterion's method",
                        check.unwrap_or_default()
                    ),
                    false,
                ),
                (EvidenceOutcome::Passed, false) => (
                    format!(
                        "STALE: the workspace changed after `{}` passed; run it again",
                        check.unwrap_or_default()
                    ),
                    false,
                ),
                (EvidenceOutcome::Failed, _) => (
                    format!(
                        "FAILED: `{}` exited {}",
                        check.unwrap_or_default(),
                        evidence
                            .exit_code
                            .map_or_else(|| "unknown".to_string(), |code| code.to_string())
                    ),
                    false,
                ),
                (EvidenceOutcome::Unavailable, _) if criterion.required => (
                    format!(
                        "UNVERIFIED and required, so completion is refused: {}",
                        single_line(&bounded(
                            evidence.detail.as_deref().unwrap_or_default(),
                            160
                        ))
                    ),
                    false,
                ),
                (EvidenceOutcome::Unavailable, _) => (
                    format!(
                        "optional, unverified (disclosed): {}",
                        single_line(&bounded(
                            evidence.detail.as_deref().unwrap_or_default(),
                            160
                        ))
                    ),
                    true,
                ),
                (EvidenceOutcome::Observed, true) => (
                    "manual observation (agent-written, not host-verified)".to_string(),
                    true,
                ),
                (EvidenceOutcome::Observed, false) => (
                    "STALE manual observation: the workspace changed after it".to_string(),
                    false,
                ),
            }
        }
        (None, Some(command)) => (format!("OPEN: run `{command}` with the shell tool"), false),
        (None, None) if !criterion.artifacts.is_empty() => (
            "OPEN: artifacts are re-read at completion; add a check or observation for content"
                .to_string(),
            false,
        ),
        (None, None) => (
            "OPEN: needs a check, a manual observation, or noCheck with a reason".to_string(),
            false,
        ),
    }
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
                    "agent-written check `{}`, approved by the user (steering {}), exited 0 on the host against the pinned artifacts (expected: {})",
                    criterion.check_command.as_deref().unwrap_or_default(),
                    criterion.approved_by_steering.as_deref().unwrap_or_default(),
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
