//! Read-only run history of a thread: whether it ever had a run, and the run that owned each
//! captured turn.

use sqlx::FromRow;

use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::parse_status;
use crate::storage::run_status_column;
use crate::storage::validate_list_limit;

/// The run a finished turn was bound to, as recorded by the host when the turn ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnRun {
    pub turn_id: String,
    pub run_id: StatefulRunId,
    pub run_status: StatefulRunStatus,
}

#[derive(FromRow)]
struct StoredTurnRun {
    turn_id: String,
    run_id: String,
    status: String,
}

impl StatefulRunStore {
    /// Whether `thread_id` was ever bound to a run, in any status.
    pub async fn thread_has_run(&self, thread_id: &str) -> Result<bool, StatefulRunStoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM stateful_run_threads WHERE thread_id = ?)",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    /// The recorded run of each finished turn of `thread_id`, newest first.
    pub async fn turn_runs_for_thread(
        &self,
        thread_id: &str,
        max_results: u32,
    ) -> Result<Vec<TurnRun>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let rows = sqlx::query_as::<_, StoredTurnRun>(concat!(
            "SELECT measurement.turn_id, measurement.run_id, ",
            run_status_column!(),
            " AS status
             FROM stateful_turn_measurements AS measurement
             JOIN stateful_runs AS run ON run.id = measurement.run_id
             WHERE measurement.thread_id = ?
             ORDER BY measurement.created_at_ms DESC, measurement.turn_id DESC LIMIT ?"
        ))
        .bind(thread_id)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(TurnRun {
                    turn_id: row.turn_id,
                    run_id: StatefulRunId::parse(row.run_id)?,
                    run_status: parse_status(&row.status)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "run_history_storage_tests.rs"]
mod tests;
