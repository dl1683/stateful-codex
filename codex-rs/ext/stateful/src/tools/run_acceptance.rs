//! The acceptance gate both completion dispositions consult before anything commits.
//!
//! A completion is accepted only when every goal sentence is covered, every required
//! criterion is satisfied by a current receipt of its host-admitted check plan (dependencies
//! included), every omission proposal is reviewed, applied steering is reconciled, no command
//! is pending, and only optional criteria are disclosed unverified. The omission pass
//! recomputes coverage on every attempt. The decision runs as a leased verification attempt
//! while the run stays Running, and the terminal transaction re-derives all of it. A refused
//! completion leaves the run Running with the exact unmet gates; required work that cannot be
//! verified ends Blocked with a partial result, never Completed. An extension cannot end a
//! turn, so the loop bound is a run transition: after `MAX_STALLED_COMPLETIONS` consecutive
//! refusals without semantic progress (durable accounting), the host moves the run to Blocked
//! with the unmet gates as its partial result, which also stops Autonomous continuation.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_stateful_runtime::AcceptanceCommit;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::CriterionVerdict;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::VerificationClaim;
use codex_stateful_runtime::ledger_verdicts;
use serde_json::json;

use super::StatefulRunUpdateTool;
use crate::StatefulEvent;
use crate::acceptance_observation::ledger_file_states;
use crate::acceptance_render::bounded;
use crate::acceptance_render::completion_basis;
use crate::tools::bounded_json_output;
use crate::tools::respond;

/// The basis of a completion under the no-tool exemption.
const NO_TOOL_BASIS: &str = "no-tool exemption: the host recorded no tool call, command, hook or other action in this run (provider-hosted calls such as web search count once observed), and the completion was its only call, so it completed without acceptance criteria; the text answer is judged by the user, not host-verified";
/// Lease of one completion verification attempt (artifact reads are bounded to seconds).
const VERIFICATION_LEASE_MS: u32 = 120_000;
/// How long completion waits for pending commands to be accounted before refusing.
const PENDING_SETTLE_WAIT: std::time::Duration = std::time::Duration::from_secs(15);
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
        local_executor: bool,
        validated_obligation_sequence: Option<u64>,
    ) -> Result<AcceptanceDecision, FunctionCallError> {
        let runtime = self.services.runtime().await.map_err(respond)?;
        // Lifecycle hooks run commands around calls and turns outside any tool call; where they
        // could have run, or where a record of an observed call was lost, this run is not
        // action-free. This call runs inside a turn whose hook set was fixed when the turn
        // started, and publishing hooks records the fact first, so every hook that can still
        // run around this completion or later in its turn is reflected here.
        if codex_extension_api::host_hooks_configured()
            || crate::host_actions::unrecorded_actions_possible()
        {
            runtime
                .record_host_action(&current.id)
                .await
                .map_err(respond)?;
        }
        // Command end events are delivered asynchronously; give commands that already exited a
        // bounded moment to have their effects accounted before judging.
        let settle_deadline = std::time::Instant::now() + PENDING_SETTLE_WAIT;
        while std::time::Instant::now() < settle_deadline
            && runtime
                .acceptance_ledger(&current.id)
                .await
                .map_err(respond)?
                .pending_commands
                > 0
        {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        // One leased verification attempt per completion; the run stays Running throughout.
        let owner = format!("completion:{}", self.thread_id);
        let attempt = runtime
            .begin_verification(&current.id, &owner, VERIFICATION_LEASE_MS)
            .await
            .map_err(respond)?;
        // The omission pass runs on every attempt: coverage is recomputed from durable state,
        // so sentences beyond one batch or a full ledger keep gating.
        let (ledger, uncovered_remaining) = runtime
            .propose_uncovered(&current.id)
            .await
            .map_err(respond)?;
        let roots = self.project_roots().await;
        let (artifacts, checkers) = if local_executor {
            ledger_file_states(&roots, &ledger).await
        } else {
            // Files are read on the app-server host, which is only the executor for the local
            // environment; elsewhere it would fingerprint same-path shadows.
            let unsupported = |files: fn(&codex_stateful_runtime::AcceptanceCriterion) -> bool| {
                ledger
                    .criteria
                    .iter()
                    .filter(|criterion| files(criterion))
                    .map(|criterion| {
                        (
                            criterion.ordinal,
                            ArtifactState::Unavailable(
                                "this turn's executor is not the local host, so file evidence is unsupported".to_string(),
                            ),
                        )
                    })
                    .collect()
            };
            (
                unsupported(|criterion| !criterion.artifacts.is_empty()),
                unsupported(|criterion| !criterion.checker.is_empty()),
            )
        };
        let commit = AcceptanceCommit {
            ledger_revision: ledger.revision,
            workspace_generation: ledger.workspace_generation,
            artifacts,
            checkers,
            verification: VerificationClaim {
                owner: owner.clone(),
                attempt,
            },
            validated_obligation_sequence,
        };
        let verdicts = ledger_verdicts(&ledger, &commit.artifacts, &commit.checkers);
        let mut unmet = verdicts
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
        if uncovered_remaining > 0 {
            unmet.push(format!(
                "{uncovered_remaining} request sentences are covered by no criterion or proposal (the ledger had no room or the batch was full); review the proposals, then complete again to receive the rest"
            ));
        }
        if ledger.pending_commands > 0 {
            unmet.push(format!(
                "{} commands of this run have not finished or their effects are not accounted for yet; wait for them before completing",
                ledger.pending_commands
            ));
        }
        let unreconciled = runtime
            .list_steering(&current.id, /*after*/ None, /*max_results*/ 100)
            .await
            .map_err(respond)?
            .into_iter()
            .filter(|steering| {
                steering.status == codex_stateful_runtime::SteeringStatus::Applied
                    && !ledger
                        .reconciled_steering
                        .iter()
                        .any(|reconciled| reconciled.steering_id == steering.id.as_str())
            })
            .map(|steering| steering.id.to_string())
            .collect::<Vec<_>>();
        if !unreconciled.is_empty() {
            unmet.push(format!(
                "applied steering {} may have changed the scope; reconcile it with stateful_acceptance_update (reconcileSteering), add criteria for any new requirement, and admit their plans again",
                unreconciled.join(", ")
            ));
        }
        if unmet.is_empty() {
            let mut basis = completion_basis(&ledger, &verdicts);
            if codex_stateful_runtime::no_tool_exempt(&ledger) {
                basis.push(NO_TOOL_BASIS.to_string());
            }
            return Ok(AcceptanceDecision::Proceed { basis, commit });
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
        Ok(AcceptanceDecision::Refused(format!(
            "completion refused; the run stays running (refusal {stalled} of {MAX_STALLED_COMPLETIONS} without acceptance progress before the host blocks it as partial). Unmet acceptance gates: {gates}. Settle each gate: repair the work and run its exact checkCommand with the shell tool, accept each omission proposal (dismiss only as covered by a user criterion), give each check its checker files and have the host admit its plan (admit). noCheck settles only optional criteria. Required work never completes unverified: if it cannot be verified or finished, set status blocked with a partial result that names the unmet criteria. Otherwise complete again"
        )))
    }

    async fn project_roots(&self) -> Vec<String> {
        match self.projects.read_project(self.project_id.clone()).await {
            Ok(Some(project)) => project.roots.into_iter().map(|root| root.path).collect(),
            Ok(None) | Err(_) => Vec::new(),
        }
    }
}

/// Most open issues one completion may declare.
const MAX_OPEN_ISSUES: usize = 16;
const MAX_OPEN_ISSUE_BYTES: usize = 1_024;

/// Every agent-admitted open issue for a completion: the declared `openIssues`, plus the
/// blockers of the final obligation (durable completion) or, without one, of the latest
/// recorded obligation. Read from structured fields only, never from narrative text; recorded
/// uncertainty that does not undermine a criterion stays allowed in a completed result.
pub(super) async fn declared_open_issues(
    open_issues: Option<&[String]>,
    final_obligation: Option<&ObligationPacket>,
    runtime: &StatefulRunStore,
    run: &StatefulRun,
) -> Result<(Vec<String>, Option<u64>), FunctionCallError> {
    let Some(declared) = open_issues else {
        return Err(FunctionCallError::RespondToModel(
            "completed requires openIssues: list every unresolved doubt, known discrepancy, failing check or open blocker that affects the result, or pass [] when there is none. Any entry ends the run blocked with a partial result instead of completed".to_string(),
        ));
    };
    if declared.len() > MAX_OPEN_ISSUES
        || declared.iter().any(|issue| {
            issue.trim().is_empty() || issue.len() > MAX_OPEN_ISSUE_BYTES || issue.contains('\0')
        })
    {
        return Err(FunctionCallError::RespondToModel(format!(
            "openIssues holds at most {MAX_OPEN_ISSUES} non-empty entries of at most {MAX_OPEN_ISSUE_BYTES} bytes"
        )));
    }
    let latest = runtime.latest_obligation(&run.id).await.map_err(respond)?;
    let sequence = latest.as_ref().map(|obligation| obligation.sequence);
    let blockers = match final_obligation {
        Some(packet) => packet.blockers.clone(),
        None => latest
            .map(|obligation| obligation.value.packet.blockers)
            .unwrap_or_default(),
    };
    Ok((
        declared
            .iter()
            .map(|issue| issue.trim().to_string())
            .chain(
                blockers
                    .into_iter()
                    .map(|blocker| format!("Recorded blocker: {blocker}")),
            )
            .collect(),
        sequence,
    ))
}

impl StatefulRunUpdateTool {
    /// Records an admitted-incomplete completion as Blocked with the partial result and the
    /// declared issues, so it can never read as done.
    pub(super) async fn block_with_open_issues(
        &self,
        current: &StatefulRun,
        expected_revision: u64,
        submitted: Option<&str>,
        open_issues: &[String],
    ) -> Result<StatefulRun, FunctionCallError> {
        let mut result = "Partial result: the completion declared unresolved issues, so the run is blocked instead of completed.\nOpen issues:".to_string();
        for issue in open_issues {
            result.push_str("\n- ");
            result.push_str(issue);
        }
        if let Some(submitted) = submitted {
            result.push_str("\n\nSubmitted result (not accepted as complete): ");
            result.push_str(submitted);
        }
        let result = bounded(&result, MAX_DURABLE_RESULT_BYTES - 4);
        self.services
            .runtime()
            .await
            .map_err(respond)?
            .update_run(
                &current.id,
                StatefulRunUpdate {
                    expected_revision,
                    status: StatefulRunStatus::Blocked,
                    strategy: current.strategy.clone(),
                    result: Some(result.trim().to_string()),
                },
            )
            .await
            .map_err(respond)
    }

    /// The tool output for a run the host recorded as blocked instead of completed.
    pub(super) fn blocked_output(
        &self,
        call: &ToolCall<'_>,
        run: &StatefulRun,
        instruction: &str,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        if let Some(event_sink) = &self.event_sink {
            event_sink.emit(StatefulEvent::RunUpdated {
                project_id: run.value.project_id.clone(),
                run_id: run.id.to_string(),
                revision: run.revision,
            });
        }
        bounded_json_output(
            call,
            json!({
                "runId": run.id.to_string(),
                "status": "blocked",
                "revision": run.revision,
                "partialResult": run.result,
                "instruction": instruction,
            }),
        )
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
