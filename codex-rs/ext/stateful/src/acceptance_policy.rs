//! Risk-proportional effort policy, read from the declared ledger structure rather than from
//! words: multi-artifact, all-or-nothing and irreversible-milestone work gets a verification
//! reserve. The reserve is advisory capacity guidance shown to the model; nothing allocates
//! it. A correct short run can finish immediately. Request coverage (the omission pass) lives
//! in the runtime, which derives it from durable run state.

use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceLedger;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::RunBudget;

/// Why a task needs reserved verification effort, read from the declared ledger.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RiskProfile {
    /// Two or more declared deliverables or artifacts.
    pub(crate) multi_artifact: bool,
    /// Two or more required criteria must all hold.
    pub(crate) all_or_nothing: bool,
    /// A criterion guards an irreversible step.
    pub(crate) irreversible: bool,
}

impl RiskProfile {
    pub(crate) fn assess(ledger: &AcceptanceLedger) -> Self {
        let active = ledger
            .criteria
            .iter()
            .filter(|criterion| criterion.state == AcceptanceState::Active)
            .collect::<Vec<_>>();
        let mut artifacts = active
            .iter()
            .flat_map(|criterion| criterion.artifacts.iter())
            .collect::<Vec<_>>();
        artifacts.sort_unstable();
        artifacts.dedup();
        let deliverables = active
            .iter()
            .filter(|criterion| criterion.kind == AcceptanceKind::Deliverable)
            .count();
        Self {
            multi_artifact: artifacts.len() >= 2 || deliverables >= 2,
            all_or_nothing: active.iter().filter(|criterion| criterion.required).count() >= 2,
            irreversible: active.iter().any(|criterion| criterion.milestone.is_some()),
        }
    }

    pub(crate) fn any(self) -> bool {
        self.multi_artifact || self.all_or_nothing || self.irreversible
    }

    pub(crate) fn labels(self) -> Vec<&'static str> {
        [
            (self.multi_artifact, "multi-artifact"),
            (self.all_or_nothing, "all-or-nothing"),
            (self.irreversible, "irreversible milestone"),
        ]
        .into_iter()
        .filter_map(|(present, label)| present.then_some(label))
        .collect()
    }
}

/// Reserved verification and rework allowance: none for low-risk work, otherwise a fifth of
/// the continuation and elapsed budgets (at least two continuations, at most half).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct VerificationReserve {
    pub(crate) continuations: u32,
    pub(crate) seconds: u32,
}

impl VerificationReserve {
    pub(crate) fn for_risk(risk: RiskProfile, budget: RunBudget) -> Self {
        if !risk.any() {
            return Self::default();
        }
        let half = budget.max_continuations / 2;
        Self {
            continuations: (budget.max_continuations / 5).max(2).min(half),
            seconds: budget.max_elapsed_seconds / 5,
        }
    }
}

#[cfg(test)]
#[path = "acceptance_policy_tests.rs"]
mod tests;
