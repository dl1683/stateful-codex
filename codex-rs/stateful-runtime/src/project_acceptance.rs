//! Inactive, bounded acceptance-ledger reads from one admitted project snapshot.

use codex_project_store::ProjectReadError;
use codex_project_store::ProjectReadSnapshot;
use thiserror::Error;

use crate::AcceptanceLedger;
use crate::StatefulRunId;
use crate::StatefulRunStoreError;

// These are storage-read budgets, not model-context or neutral-API page budgets.
const MAX_LEDGER_BYTES: i64 = 1024 * 1024;
const MAX_PENDING_COMMANDS: i64 = 4096;

#[derive(Debug, Error)]
pub enum ProjectAcceptanceReadError {
    #[error(transparent)]
    Snapshot(#[from] ProjectReadError),
    #[error(transparent)]
    Runtime(#[from] StatefulRunStoreError),
    #[error("acceptance run does not belong to the admitted project")]
    ProjectMismatch,
    #[error("acceptance ledger exceeds the complete-read budget")]
    ReadLimit,
}

/// Read the complete bounded ledger and finish the consuming snapshot. Requires
/// the actual compiled runtime domain and exact project/run binding. No ledger
/// or request parts are created; acceptance_request remains a write operation.
/// This is trusted domain access, not an effective-memory or neutral read API.
///
/// Complete-read limits: one ledger, 32 criteria, 128 evidence-history rows,
/// 64 reconciliations, 4,096 pending commands and 1 MiB aggregate stored field
/// bytes (including numeric fields and sort keys). Legacy writers can produce
/// valid populations beyond these budgets. Those refuse with ReadLimit; unlike
/// the legacy reader, excess reconciliations are never silently truncated.
///
/// Refusal/cancellation drops the snapshot and leaves its owner close-only;
/// await close before reopening. No native routing or acceptance is activated.
pub async fn read_project_acceptance_ledger(
    mut snapshot: ProjectReadSnapshot<'_>,
    run_id: &StatefulRunId,
) -> Result<AcceptanceLedger, ProjectAcceptanceReadError> {
    snapshot.require_domain(&crate::project_store_domain().map_err(ProjectReadError::from)?)?;
    let project = snapshot.identity().project_uuid.to_string();
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT CASE WHEN typeof(project_id)='text' AND octet_length(project_id)=36
         THEN project_id END FROM stateful_runs WHERE id=?",
    )
    .bind(run_id.as_str())
    .fetch_optional(snapshot.connection())
    .await
    .map_err(StatefulRunStoreError::from)?
    .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
    if owner.as_deref() != Some(project.as_str()) {
        return Err(ProjectAcceptanceReadError::ProjectMismatch);
    }
    // Bound metadata work and payload allocation before the shared decoder. Each
    // indexed scan reads at most its row cap plus one; overflow refuses, never
    // hides a criterion, newest failure, reconciliation or pending command.
    let mut bytes = 0_i64;
    for (query, limit) in [
        ("SELECT COALESCE(SUM(bytes),0), COUNT(*) FROM (
            SELECT octet_length(revision)+octet_length(workspace_generation)
                +octet_length(observed_executions)+octet_length(stalled_completions)
                +octet_length(verification_attempt)
                +COALESCE(octet_length(verification_lease_expires_at_ms),0) AS bytes
            FROM stateful_acceptance_ledgers WHERE run_id=? LIMIT ?)", 1_i64),
        ("SELECT COALESCE(SUM(bytes),0), COUNT(*) FROM (
            SELECT octet_length(ordinal)+octet_length(criterion_id)+octet_length(origin)+octet_length(kind)
                +octet_length(state)+octet_length(statement)+octet_length(requirement)
                +octet_length(required)+octet_length(revision)+octet_length(ledger_revision)
                +COALESCE(octet_length(span_start),0)+COALESCE(octet_length(span_end),0)
                +COALESCE(octet_length(plan_criterion_revision),0)
                +COALESCE(octet_length(plan_ledger_revision),0)+COALESCE(octet_length(dismissal_covered_by),0)
                +octet_length(depends_on_json)+COALESCE(octet_length(milestone),0)
                +COALESCE(octet_length(expected_observation),0)+octet_length(artifacts_json)
                +octet_length(checker_json)+COALESCE(octet_length(check_command),0)
                +COALESCE(octet_length(check_cwd),0)+COALESCE(octet_length(plan_checker_digest),0)
                +COALESCE(octet_length(note),0) AS bytes
            FROM stateful_acceptance_criteria WHERE run_id=? LIMIT ?)", 32_i64),
        ("SELECT COALESCE(SUM(bytes),0), COUNT(*) FROM (
            SELECT octet_length(sequence)+octet_length(ordinal)+octet_length(source)+octet_length(outcome)+octet_length(source_id)
                +octet_length(workspace_generation)+octet_length(criterion_revision)+octet_length(observed_at_ms)
                +COALESCE(octet_length(exit_code),0)
                +COALESCE(octet_length(command),0)+COALESCE(octet_length(output_tail),0)
                +COALESCE(octet_length(output_digest),0)+COALESCE(octet_length(artifact_digest),0)
                +COALESCE(octet_length(checker_digest),0)+COALESCE(octet_length(detail),0) AS bytes
            FROM stateful_acceptance_evidence WHERE run_id=? LIMIT ?)", 128_i64),
        ("SELECT COALESCE(SUM(bytes),0), COUNT(*) FROM (
            SELECT octet_length(steering_id)+octet_length(reason)+octet_length(reconciled_at_ms) AS bytes
            FROM stateful_acceptance_steering WHERE run_id=? LIMIT ?)", 64_i64),
        ("SELECT 0, COUNT(*) FROM (
            SELECT 1 FROM stateful_acceptance_pending WHERE run_id=? LIMIT ?)", MAX_PENDING_COMMANDS),
    ] {
        let (size, count): (i64, i64) = sqlx::query_as(query)
            .bind(run_id.as_str())
            .bind(limit + 1)
            .fetch_one(snapshot.connection())
            .await
            .map_err(StatefulRunStoreError::from)?;
        bytes = bytes.saturating_add(size);
        if count > limit || bytes > MAX_LEDGER_BYTES {
            return Err(ProjectAcceptanceReadError::ReadLimit);
        }
    }
    let ledger = crate::acceptance_storage::load_ledger(snapshot.connection(), run_id).await?;
    snapshot.finish().await?;
    Ok(ledger)
}

#[cfg(test)]
#[path = "project_acceptance_tests.rs"]
mod tests;
