//! The host-decided no-tool exemption. A run whose turns made no tool call at all and only
//! produced a text answer completes without an acceptance ledger; any other action brings
//! E's full ledger path back.
//!
//! The decision rests only on durable facts the host recorded before each action could run,
//! never on the model's claim or the request's wording:
//! - the host records an action against the thread's open runs (`side_effects`) before a model
//!   call item reaches dispatch (hosted ones as soon as they are observed), before a user shell
//!   command is spawned, and before a completion that lifecycle hooks could surround or that
//!   follows a lost call record; a lone completion call is the only call it leaves out;
//! - the host records a completion attempt (`completion_attempts`) before it releases a lone
//!   completion call; only the run's sole attempt, the completion being judged, is exempt, so a
//!   rejected earlier attempt brings the ledger back;
//! - the run was created by this process (`observation_version` holds the creating process's
//!   epoch), so every one of its actions passed through this process's fences. A restart, a
//!   run created elsewhere or before observation existed is not exempt;
//! - the ledger declares no criterion and saw no command, mutation or pending command.
//!
//! The answer is judged by the user; it is not host-verified. Open issues, recorded blockers,
//! unresolved or unreconciled steering and E's other gates still apply.

use std::hash::BuildHasher;
use std::hash::Hasher;
use std::sync::LazyLock;

use sqlx::SqliteConnection;

use crate::AcceptanceLedger;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::unix_timestamp_millis;

/// This process's observer epoch: a random value of at least 2, so it never equals the
/// legacy markers 0 (began before observation) and 1 (observed under an earlier rule).
static OBSERVER_EPOCH: LazyLock<i64> = LazyLock::new(|| {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos()),
    );
    i64::try_from(hasher.finish() >> 2).unwrap_or_default() + 2
});

/// Whether the host may complete the run without acceptance criteria: nothing but the
/// completion being judged, its sole recorded attempt, happened in the run.
pub fn no_tool_exempt(ledger: &AcceptanceLedger) -> bool {
    action_free(ledger) && ledger.completion_attempts == 1
}

/// Whether the run may still end under the exemption: no action and no completion attempt yet.
pub fn no_tool_eligible(ledger: &AcceptanceLedger) -> bool {
    action_free(ledger) && ledger.completion_attempts == 0
}

fn action_free(ledger: &AcceptanceLedger) -> bool {
    ledger.observed_by_this_process
        && ledger.host_actions == 0
        && ledger.criteria.is_empty()
        && ledger.observed_executions == 0
        && ledger.workspace_generation == 0
        && ledger.pending_commands == 0
}

pub(crate) fn observed_by_this_process(observation_version: i64) -> bool {
    observation_version == *OBSERVER_EPOCH
}

impl StatefulRunStore {
    /// Records one action of the run before it runs. Terminal runs ignore it.
    pub async fn record_host_action(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<(), StatefulRunStoreError> {
        self.adjust_counters(run_id, "side_effects = side_effects + 1")
            .await
    }

    /// Records one action against every nonterminal run bound to `thread_id` now, in one
    /// statement, before the action runs. A thread without such a run records nothing.
    pub async fn record_host_action_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<(), StatefulRunStoreError> {
        self.record_for_thread(thread_id, "side_effects = side_effects + 1")
            .await
    }

    /// Records one completion attempt against every nonterminal run bound to `thread_id` now,
    /// before a lone completion call is released to dispatch.
    pub async fn record_completion_attempt_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<(), StatefulRunStoreError> {
        self.record_for_thread(thread_id, "completion_attempts = completion_attempts + 1")
            .await
    }

    async fn record_for_thread(
        &self,
        thread_id: &str,
        assignment: &'static str,
    ) -> Result<(), StatefulRunStoreError> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE stateful_acceptance_ledgers
             SET {assignment}, updated_at_ms = ?
             WHERE run_id IN (
                 SELECT run.id FROM stateful_runs AS run
                 JOIN stateful_run_threads AS thread ON thread.run_id = run.id
                 WHERE thread.thread_id = ?
                   AND run.status NOT IN ('completed', 'cancelled', 'failed')
             )"
        )))
        .bind(unix_timestamp_millis()?)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// Creates the ledger of a new run, stamped with this process's epoch, inside its creating
/// transaction, so observation covers the run from its first instant.
pub(crate) async fn begin_observation(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "INSERT INTO stateful_acceptance_ledgers (
            run_id, revision, workspace_generation, observed_executions, stalled_completions,
            stalled_fingerprint, verification_attempt, updated_at_ms, observation_version
         ) VALUES (?, 0, 0, 0, 0, '', 0, ?, ?)",
    )
    .bind(run_id.as_str())
    .bind(now)
    .bind(*OBSERVER_EPOCH)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Records, inside the terminal transaction, that the run completed under the exemption.
/// The stored value `readOnly` predates this rule; a run without any action is read-only.
pub(crate) async fn record_exemption(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query("UPDATE stateful_acceptance_ledgers SET exemption = 'readOnly' WHERE run_id = ?")
        .bind(run_id.as_str())
        .execute(&mut *connection)
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "acceptance_exemption_tests.rs"]
mod tests;
