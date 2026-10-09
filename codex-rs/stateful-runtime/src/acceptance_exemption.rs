//! The host-decided read-only exemption. A run that produced no artifact, changed nothing and
//! took no action with possible effects (a read-only answer, lookup or explanation) completes
//! without an acceptance ledger. The decision rests only on durable facts the host observed:
//! the ledger declares no criterion, no side effect or workspace mutation was recorded,
//! nothing is pending, and the host observed the run's effects since it began. The request's
//! wording is never authority, and the model cannot assert the exemption; criteria re-enter
//! only as structured ledger entries, and declaring any criterion ends it.
//!
//! Read-only answers, including substantial reasoning answered entirely in chat, are judged by
//! the user: acceptance criteria stated only in prose for such an answer are not host-verified.
//! Open issues and recorded blockers still prevent `Completed`.

use sqlx::SqliteConnection;

use crate::AcceptanceLedger;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;

/// Whether the host may complete the run without acceptance criteria.
pub fn read_only_exempt(ledger: &AcceptanceLedger) -> bool {
    ledger.observations_complete
        && ledger.criteria.is_empty()
        && ledger.side_effects == 0
        && ledger.workspace_generation == 0
        && ledger.pending_commands == 0
}

impl StatefulRunStore {
    /// Records one host-observed side-effecting action of the run. Terminal runs ignore it.
    pub async fn record_side_effect(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<(), StatefulRunStoreError> {
        self.adjust_counters(run_id, "side_effects = side_effects + 1")
            .await
    }
}

/// Records, inside the terminal transaction, that the run completed under the exemption.
pub(crate) async fn record_exemption(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<(), StatefulRunStoreError> {
    crate::acceptance_storage::ensure_ledger(connection, run_id).await?;
    sqlx::query("UPDATE stateful_acceptance_ledgers SET exemption = 'readOnly' WHERE run_id = ?")
        .bind(run_id.as_str())
        .execute(&mut *connection)
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "acceptance_exemption_tests.rs"]
mod tests;
