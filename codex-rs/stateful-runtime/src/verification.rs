//! Shared verification-lease policy; callers own transaction and actor admission.
use sqlx::SqliteConnection;

use crate::StatefulRunId;
use crate::StatefulRunStoreError;
use crate::acceptance::AcceptanceError;
use crate::acceptance_storage::ensure_ledger;
use crate::acceptance_storage::load_ledger;
use crate::storage::load_run;
use crate::storage::unix_timestamp_millis;

pub(crate) async fn begin_on(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    owner: &str,
    lease_ms: u32,
) -> Result<u64, StatefulRunStoreError> {
        let run = load_run(&mut *connection, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        ensure_ledger(&mut *connection, run_id).await?;
        let now = unix_timestamp_millis()?;
        let ledger = load_ledger(&mut *connection, run_id).await?;
        let held_by_other = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM stateful_acceptance_ledgers
             WHERE run_id = ? AND verification_owner IS NOT NULL AND verification_owner <> ?",
        )
        .bind(run_id.as_str())
        .bind(owner)
        .fetch_optional(&mut *connection)
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
        .execute(&mut *connection)
        .await?;
        let ledger = load_ledger(&mut *connection, run_id).await?;
        Ok(ledger.verification_attempt)
}

pub(crate) async fn end_on(
    connection: &mut SqliteConnection,
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
        .execute(connection)
        .await?;
        Ok(())
    }

