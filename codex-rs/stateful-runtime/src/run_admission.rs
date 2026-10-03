//! Durable run admission: each turn of a thread is admitted against the thread's run binding
//! before it samples, so a continuation (a keeper's `exec resume`, a restart) is attributed to
//! the same run as the turns before it, and a terminal run is never silently reopened.

use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::load_run;
use crate::storage::unix_timestamp_millis;

/// The run a turn was admitted under, and the thread binding generation at that time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunTurnAdmission {
    pub turn_id: String,
    pub thread_id: String,
    pub run_id: StatefulRunId,
    pub generation: u64,
}

impl StatefulRunStore {
    /// Admits `turn_id` of `thread_id` to a run, durably, and returns the admission with the run.
    ///
    /// The thread's bound run is kept while it is not terminal and belongs to `project_id`,
    /// even when another run of the thread was updated more recently; otherwise the thread is
    /// bound to its newest active run of the project, advancing the binding generation. A turn
    /// admitted before (a retry, a restart) keeps its original admission. `None` when the thread
    /// has no active run of the project: a terminal run is never reopened.
    pub async fn admit_run_turn(
        &self,
        project_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Option<(RunTurnAdmission, StatefulRun)>, StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some((run_id, generation)) = sqlx::query_as::<_, (String, i64)>(
            "SELECT run_id, generation FROM stateful_run_turn_admissions
             WHERE turn_id = ? AND thread_id = ?",
        )
        .bind(turn_id)
        .bind(thread_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            let run_id = StatefulRunId::parse(run_id)?;
            let run = load_run(&mut transaction, &run_id).await?;
            transaction.commit().await?;
            // A replay under another project is not this turn's admission; the original stays.
            return Ok(run
                .filter(|run| run.value.project_id == project_id)
                .map(|run| {
                    (
                        RunTurnAdmission {
                            turn_id: turn_id.to_string(),
                            thread_id: thread_id.to_string(),
                            run_id,
                            generation: u64::try_from(generation).unwrap_or_default(),
                        },
                        run,
                    )
                }));
        }
        let bound = sqlx::query_as::<_, (String, i64)>(
            "SELECT run_id, generation FROM stateful_thread_run_bindings WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let bound_run = match &bound {
            Some((run_id, _)) => {
                load_run(&mut transaction, &StatefulRunId::parse(run_id.clone())?).await?
            }
            None => None,
        }
        .filter(|run| !run.status.is_terminal() && run.value.project_id == project_id);
        let run = match bound_run {
            Some(run) => run,
            None => {
                let newest = sqlx::query_scalar::<_, String>(
                    "SELECT run.id FROM stateful_runs AS run
                     JOIN stateful_run_threads AS thread ON thread.run_id = run.id
                     WHERE thread.thread_id = ? AND run.project_id = ?
                       AND run.status NOT IN ('completed', 'cancelled', 'failed')
                     ORDER BY run.updated_at_ms DESC, run.id LIMIT 1",
                )
                .bind(thread_id)
                .bind(project_id)
                .fetch_optional(&mut *transaction)
                .await?;
                let Some(newest) = newest else {
                    transaction.commit().await?;
                    return Ok(None);
                };
                let Some(run) = load_run(&mut transaction, &StatefulRunId::parse(newest)?).await?
                else {
                    transaction.commit().await?;
                    return Ok(None);
                };
                run
            }
        };
        let now = unix_timestamp_millis()?;
        let generation = match &bound {
            Some((run_id, generation)) if run_id == run.id.as_str() => *generation,
            Some((_, generation)) => {
                let next = generation.saturating_add(1);
                sqlx::query(
                    "UPDATE stateful_thread_run_bindings
                     SET run_id = ?, generation = ?, updated_at_ms = ? WHERE thread_id = ?",
                )
                .bind(run.id.as_str())
                .bind(next)
                .bind(now)
                .bind(thread_id)
                .execute(&mut *transaction)
                .await?;
                next
            }
            None => {
                sqlx::query(
                    "INSERT INTO stateful_thread_run_bindings (thread_id, run_id, generation, updated_at_ms)
                     VALUES (?, ?, 1, ?)",
                )
                .bind(thread_id)
                .bind(run.id.as_str())
                .bind(now)
                .execute(&mut *transaction)
                .await?;
                1
            }
        };
        sqlx::query(
            "INSERT INTO stateful_run_turn_admissions (turn_id, thread_id, run_id, generation, created_at_ms)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(turn_id)
        .bind(thread_id)
        .bind(run.id.as_str())
        .bind(generation)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(Some((
            RunTurnAdmission {
                turn_id: turn_id.to_string(),
                thread_id: thread_id.to_string(),
                run_id: run.id.clone(),
                generation: u64::try_from(generation).unwrap_or_default(),
            },
            run,
        )))
    }
}

#[cfg(test)]
#[path = "run_admission_tests.rs"]
mod tests;
