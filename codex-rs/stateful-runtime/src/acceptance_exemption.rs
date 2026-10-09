//! The host-decided read-only exemption. A run that produced no artifact, changed nothing and
//! took no action with possible external effects (a read-only answer, lookup or explanation)
//! completes without an acceptance ledger. The decision is the host's, from durable facts it
//! observed: the ledger holds no criterion, no side effect and no workspace mutation was
//! recorded, nothing is pending, and the request (goal and applied steering) states no
//! acceptance criteria. The model cannot assert the exemption; adding any criterion ends it.
//!
//! Substantial reasoning answered entirely in chat stays exempt by definition: it has no
//! artifact or effect for the ledger to check, so the user judges it. Open issues and recorded
//! blockers still prevent `Completed`.

use sqlx::SqliteConnection;

use crate::AcceptanceLedger;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;

/// Words that state an acceptance criterion; any one of them in the request ends the
/// exemption. Deliberately broad: a missed criterion would let effect-free work complete
/// unchecked, while a false hit only asks for the ledger.
const CRITERIA_WORDS: &[&str] = &[
    "acceptance",
    "criteria",
    "criterion",
    "must",
    "required",
    "requirement",
    "requirements",
    "ensure",
    "verify",
    "verified",
    "validate",
    "validated",
];

/// Whether the host may complete the run without acceptance criteria.
pub fn read_only_exempt(request: &str, ledger: &AcceptanceLedger) -> bool {
    ledger.criteria.is_empty()
        && ledger.side_effects == 0
        && ledger.workspace_generation == 0
        && ledger.pending_commands == 0
        && !states_criteria(request)
}

/// Whether the request states acceptance criteria (a criteria word, or "make sure").
pub fn states_criteria(request: &str) -> bool {
    let words = request
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    words
        .iter()
        .any(|word| CRITERIA_WORDS.contains(&word.as_str()))
        || words
            .windows(2)
            .any(|pair| pair[0] == "make" && pair[1] == "sure")
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
