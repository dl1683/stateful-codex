//! The acceptance gate both completion dispositions consult before anything commits.
//!
//! A completion is accepted only when every required criterion of the run's ledger is
//! satisfied with current evidence (dependencies included), every omission proposal is
//! reviewed, and only optional criteria are disclosed unverified. Substantial tasks first get
//! the one-time host omission check. The decision runs as a leased verification attempt while
//! the run stays Running, and the terminal transaction consumes that attempt. A refused
//! completion leaves the run Running with the exact unmet gates; required work that cannot be
//! verified ends Blocked with a partial result, never Completed. An extension cannot end a
//! turn, so the loop bound is a run transition: after `MAX_STALLED_COMPLETIONS` consecutive
//! refusals with no ledger progress (durable accounting), the host moves the run to Blocked
//! with the unmet gates as its partial result, which also stops Autonomous continuation.

use codex_extension_api::FunctionCallError;
use codex_stateful_runtime::AcceptanceCommit;
use codex_stateful_runtime::CriterionVerdict;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::ledger_verdicts;

use super::StatefulRunUpdateTool;
use crate::acceptance_observation::ledger_artifact_states;
use crate::acceptance_policy::MAX_OMISSION_PROPOSALS;
use crate::acceptance_policy::is_substantial;
use crate::acceptance_policy::omission_proposals;
use crate::acceptance_render::bounded;
use crate::acceptance_render::completion_basis;
use crate::tools::respond;

/// Lease of one completion verification attempt (artifact reads are bounded to seconds).
const VERIFICATION_LEASE_MS: u32 = 120_000;
/// Consecutive refused completions without ledger progress before the host blocks the run.
pub(super) const MAX_STALLED_COMPLETIONS: u32 = 3;
/// The runtime's bound on a stored run result.
const MAX_DURABLE_RESULT_BYTES: usize = 32 * 1024;
/// Bound on the unmet-gate list in one refusal; the exact ledger is a paged read away.
const MAX_GATE_LIST_BYTES: usize = 2_400;
const MAX_GATE_REASON_BYTES: usize = 320;

pub(super) enum AcceptanceDecision {
    /// Commit with this validated snapshot; `basis` lines are disclosed in the result.
    Proceed {
        commit: AcceptanceCommit,
        basis: Vec<String>,
    },
    /// The run stays Running; the message lists the unmet gates and how to settle each.
    Refused(String),
    /// The host blocked the run as a partial result after repeated stalled completions.
    Blocked(Box<StatefulRun>),
}

impl StatefulRunUpdateTool {
    pub(super) async fn acceptance_decision(
        &self,
        current: &StatefulRun,
        submitted: Option<&str>,
    ) -> Result<AcceptanceDecision, FunctionCallError> {
        let runtime = self.services.runtime().await.map_err(respond)?;
        // One leased verification attempt per completion; the run stays Running throughout.
        let owner = format!("completion:{}", self.thread_id);
        let attempt = runtime
            .begin_verification(&current.id, &owner, VERIFICATION_LEASE_MS)
            .await
            .map_err(respond)?;
        let mut ledger = runtime
            .acceptance_ledger(&current.id)
            .await
            .map_err(respond)?;
        let substantial = is_substantial(&current.value.goal, &ledger);
        let mut uncovered_beyond_cap = 0;
        if substantial && !ledger.omission_checked {
            let (proposals, beyond) = omission_proposals(&current.value.goal, &ledger);
            uncovered_beyond_cap = beyond;
            ledger = runtime
                .record_omission_check(&current.id, proposals)
                .await
                .map_err(respond)?;
        }
        let roots = self.project_roots().await;
        let commit = AcceptanceCommit {
            ledger_revision: ledger.revision,
            workspace_generation: ledger.workspace_generation,
            artifacts: ledger_artifact_states(&roots, &ledger).await,
            omission_required: substantial,
            verification: Some((owner.clone(), attempt)),
        };
        let verdicts = ledger_verdicts(&ledger, &commit);
        let unmet = verdicts
            .iter()
            .filter_map(|(ordinal, verdict)| match verdict {
                CriterionVerdict::Unmet(reason) => Some(gate_line(&ledger, *ordinal, reason)),
                CriterionVerdict::SatisfiedByHost
                | CriterionVerdict::ArtifactsPresent
                | CriterionVerdict::ManualObservation
                | CriterionVerdict::DisclosedUnverified(_)
                | CriterionVerdict::Closed(_) => None,
            })
            .collect::<Vec<_>>();
        if unmet.is_empty() {
            return Ok(AcceptanceDecision::Proceed {
                basis: completion_basis(&ledger, &verdicts),
                commit,
            });
        }
        let gates = gate_list(&unmet);
        runtime
            .end_verification(&current.id, &owner, attempt)
            .await
            .map_err(respond)?;
        let stalled = runtime
            .note_rejected_completion(&current.id)
            .await
            .map_err(respond)?;
        if stalled >= MAX_STALLED_COMPLETIONS {
            let mut result = format!(
                "Partial result: completion was refused {stalled} times in a row without acceptance progress, so the host stopped this run as blocked instead of done. Unmet acceptance gates: {gates}"
            );
            if let Some(submitted) = submitted {
                result.push_str("\n\nLast submitted result (not accepted): ");
                result.push_str(submitted);
            }
            let result = bounded(&result, MAX_DURABLE_RESULT_BYTES - 4);
            if let Ok(run) = runtime
                .update_run(
                    &current.id,
                    StatefulRunUpdate {
                        expected_revision: current.revision,
                        status: StatefulRunStatus::Blocked,
                        strategy: current.strategy.clone(),
                        result: Some(result.trim().to_string()),
                    },
                )
                .await
            {
                return Ok(AcceptanceDecision::Blocked(Box::new(run)));
            }
        }
        let omission_note = if uncovered_beyond_cap > 0 {
            format!(
                " The omission check proposed the first {MAX_OMISSION_PROPOSALS} uncovered requirement sentences of the goal; {uncovered_beyond_cap} more were not proposed, so review the goal for them too."
            )
        } else {
            String::new()
        };
        Ok(AcceptanceDecision::Refused(format!(
            "completion refused; the run stays running (refusal {stalled} of {MAX_STALLED_COMPLETIONS} without acceptance progress before the host blocks it as partial). Unmet acceptance gates: {gates}.{omission_note} Settle each gate: repair the work and run its exact checkCommand with the shell tool, and use stateful_acceptance_update to accept or dismiss (with a reason) each omission proposal. noCheck settles only optional criteria. Required work never completes unverified: if it cannot be verified or finished, set status blocked with a partial result that names the unmet criteria. Otherwise complete again"
        )))
    }

    async fn project_roots(&self) -> Vec<String> {
        match self.projects.read_project(self.project_id.clone()).await {
            Ok(Some(project)) => project.roots.into_iter().map(|root| root.path).collect(),
            Ok(None) | Err(_) => Vec::new(),
        }
    }
}

/// Appends the disclosed acceptance basis to a durable result, within the stored bound.
pub(super) fn with_acceptance_basis(result: String, basis: &[String]) -> String {
    // The header and the overflow line must fit; otherwise the basis stays in the ledger
    // and the completion output only.
    if basis.is_empty() || result.len() + 240 > MAX_DURABLE_RESULT_BYTES {
        return result;
    }
    let mut durable = result;
    durable.push_str("\n\nAcceptance basis:");
    let mut omitted = 0;
    for (index, line) in basis.iter().enumerate() {
        let remaining = basis.len() - index;
        let reserve = if remaining > 1 { 120 } else { 0 };
        if durable.len() + line.len() + 3 + reserve > MAX_DURABLE_RESULT_BYTES {
            omitted = remaining;
            break;
        }
        durable.push_str("\n- ");
        durable.push_str(line);
    }
    if omitted > 0 {
        durable.push_str(&format!(
            "\n- {omitted} more criteria: read the ledger with stateful_run_read section \"acceptance\"."
        ));
    }
    durable
}

fn gate_line(
    ledger: &codex_stateful_runtime::AcceptanceLedger,
    ordinal: u32,
    reason: &str,
) -> String {
    let quote = ledger
        .criterion(ordinal)
        .map(|criterion| bounded(&criterion.statement, 120))
        .unwrap_or_default();
    bounded(
        &format!("C{ordinal} ({quote}): {reason}"),
        MAX_GATE_REASON_BYTES,
    )
}

fn gate_list(unmet: &[String]) -> String {
    let mut list = String::new();
    for (index, gate) in unmet.iter().enumerate() {
        if list.len() + gate.len() + 2 > MAX_GATE_LIST_BYTES {
            list.push_str(&format!(
                "; and {} more (stateful_run_read section \"acceptance\")",
                unmet.len() - index
            ));
            break;
        }
        if !list.is_empty() {
            list.push_str("; ");
        }
        list.push_str(gate);
    }
    list
}
