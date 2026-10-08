//! Acceptance bookkeeping around executions and completion attempts: durable pending-command
//! accounting, cold re-entry invalidation, the leased verification attempt, and the
//! no-progress guard keyed by a semantic progress fingerprint.

use std::hash::Hash;
use std::hash::Hasher;

use serde_json::json;
use sqlx::SqliteConnection;

use crate::AcceptanceLedger;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::acceptance::AcceptanceError;
use crate::acceptance_storage::ensure_ledger;
use crate::acceptance_storage::load_ledger;
use crate::storage::load_run;
use crate::storage::unix_timestamp_millis;

impl StatefulRunStore {
    /// A command of the run has started; until `finish_command` accounts for its effects the
    /// terminal gate refuses completion. Terminal runs ignore it.
    pub async fn begin_command(
        &self,
        run_id: &StatefulRunId,
        call_id: &str,
    ) -> Result<(), StatefulRunStoreError> {
        crate::run::validate_source_id(call_id)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let Some(run) = load_run(&mut transaction, run_id).await? else {
            return Ok(());
        };
        if run.status.is_terminal() {
            return Ok(());
        }
        ensure_ledger(&mut transaction, run_id).await?;
        sqlx::query(
            "INSERT OR IGNORE INTO stateful_acceptance_pending (run_id, call_id, started_at_ms)
             VALUES (?, ?, ?)",
        )
        .bind(run_id.as_str())
        .bind(call_id)
        .bind(unix_timestamp_millis()?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// A started command's effects (evidence, execution count, mutation) are durably
    /// accounted for, or the host proved it never launched.
    pub async fn finish_command(
        &self,
        run_id: &StatefulRunId,
        call_id: &str,
    ) -> Result<(), StatefulRunStoreError> {
        sqlx::query("DELETE FROM stateful_acceptance_pending WHERE run_id = ? AND call_id = ?")
            .bind(run_id.as_str())
            .bind(call_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Cold re-entry of a run (a new process): commands of the previous process ended with
    /// it and their effects are unknown, and mutations while no observer ran are unknown, so
    /// pending entries are cleared and every earlier receipt and observation becomes stale.
    pub async fn invalidate_after_reentry(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<(), StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let has_state = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM stateful_acceptance_evidence WHERE run_id = ?
             UNION ALL SELECT 1 FROM stateful_acceptance_pending WHERE run_id = ? LIMIT 1",
        )
        .bind(run_id.as_str())
        .bind(run_id.as_str())
        .fetch_optional(&mut *transaction)
        .await?
        .is_some();
        if !has_state {
            transaction.commit().await?;
            return Ok(());
        }
        sqlx::query("DELETE FROM stateful_acceptance_pending WHERE run_id = ?")
            .bind(run_id.as_str())
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET workspace_generation = workspace_generation + 1, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(unix_timestamp_millis()?)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Starts a completion verification attempt and leases it to `owner`. The run's public
    /// status stays `Running`; another owner's attempt waits until the lease ends or expires.
    pub async fn begin_verification(
        &self,
        run_id: &StatefulRunId,
        owner: &str,
        lease_ms: u32,
    ) -> Result<u64, StatefulRunStoreError> {
        crate::run::validate_source_id(owner)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        ensure_ledger(&mut transaction, run_id).await?;
        let now = unix_timestamp_millis()?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        let held_by_other = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM stateful_acceptance_ledgers
             WHERE run_id = ? AND verification_owner IS NOT NULL AND verification_owner <> ?",
        )
        .bind(run_id.as_str())
        .bind(owner)
        .fetch_optional(&mut *transaction)
        .await?
        .is_some();
        // The same owner (one thread's completion path) takes over its own earlier attempt,
        // so an attempt abandoned by an error does not lock out its retry.
        if let Some(expires) = ledger.verification_lease_expires_at_ms
            && expires > now
            && held_by_other
        {
            return Err(AcceptanceError::VerificationLeased(expires).into());
        }
        let expires = now
            .checked_add(i64::from(lease_ms))
            .ok_or(StatefulRunStoreError::TimestampOverflow)?;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET verification_attempt = verification_attempt + 1, verification_owner = ?,
                 verification_lease_expires_at_ms = ?, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(owner)
        .bind(expires)
        .bind(now)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger.verification_attempt)
    }

    /// Ends a verification attempt without completing (a refusal); only its owner can end it.
    pub async fn end_verification(
        &self,
        run_id: &StatefulRunId,
        owner: &str,
        attempt: u64,
    ) -> Result<(), StatefulRunStoreError> {
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET verification_owner = NULL, verification_lease_expires_at_ms = NULL
             WHERE run_id = ? AND verification_owner = ? AND verification_attempt = ?",
        )
        .bind(run_id.as_str())
        .bind(owner)
        .bind(i64::try_from(attempt).map_err(|_| StatefulRunStoreError::CountOverflow)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Counts a rejected completion. The count is consecutive rejections at the same semantic
    /// progress fingerprint: appends that change nothing material (a repeated identical
    /// observation, the same failing check rerun on the same state) do not reset it.
    pub async fn note_rejected_completion(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<u32, StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if load_run(&mut transaction, run_id).await?.is_none() {
            return Err(StatefulRunStoreError::RunNotFound(run_id.to_string()));
        }
        ensure_ledger(&mut transaction, run_id).await?;
        let fingerprint = progress_fingerprint(&load_ledger(&mut transaction, run_id).await?);
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET stalled_completions = CASE WHEN stalled_fingerprint = ?
                     THEN stalled_completions + 1 ELSE 1 END,
                 stalled_fingerprint = ?, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(&fingerprint)
        .bind(&fingerprint)
        .bind(unix_timestamp_millis()?)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger.stalled_completions)
    }
}

/// What acceptance progress means: criterion terms and states, plan admissions, the
/// verdict-relevant fields of each criterion's newest evidence, the workspace generation and
/// reconciled steering. Sequences, timestamps, source IDs and output text are excluded.
fn progress_fingerprint(ledger: &AcceptanceLedger) -> String {
    let criteria = ledger
        .criteria
        .iter()
        .map(|criterion| {
            json!({
                "ordinal": criterion.ordinal,
                "revision": criterion.revision,
                "state": criterion.state,
                "plan": criterion.plan,
                "evidence": criterion.evidence.as_ref().map(|evidence| json!({
                    "source": evidence.source,
                    "outcome": evidence.outcome,
                    "exitCode": evidence.exit_code,
                    "artifactDigest": evidence.artifact_digest,
                    "checkerDigest": evidence.checker_digest,
                    "detail": evidence.detail,
                    "generation": evidence.workspace_generation,
                    "criterionRevision": evidence.criterion_revision,
                })),
            })
        })
        .collect::<Vec<_>>();
    let state = json!({
        "criteria": criteria,
        "generation": ledger.workspace_generation,
        "steering": ledger.reconciled_steering,
    });
    // A changed hasher across toolchains only restarts the count once; it never skips one.
    let mut hasher = std::hash::DefaultHasher::new();
    state.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The terminal transaction consumes the attempt it was validated in; an expired or replaced
/// lease means another attempt may have observed different state.
pub(crate) async fn consume_verification_lease(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    owner: &str,
    attempt: u64,
) -> Result<(), StatefulRunStoreError> {
    let consumed = sqlx::query(
        "UPDATE stateful_acceptance_ledgers
         SET verification_owner = NULL, verification_lease_expires_at_ms = NULL
         WHERE run_id = ? AND verification_owner = ? AND verification_attempt = ?
           AND verification_lease_expires_at_ms > ?",
    )
    .bind(run_id.as_str())
    .bind(owner)
    .bind(i64::try_from(attempt).map_err(|_| StatefulRunStoreError::CountOverflow)?)
    .bind(unix_timestamp_millis()?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if consumed != 1 {
        return Err(StatefulRunStoreError::AcceptanceChanged);
    }
    Ok(())
}
