//! Read-only run history used to label captured turns and report the latest project run.

use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::load_run;
use crate::storage::validate_list_limit;

impl StatefulRunStore {
    /// Every run attached to `thread_id`, terminal ones included, newest first.
    pub async fn runs_for_thread(
        &self,
        thread_id: &str,
        max_results: u32,
    ) -> Result<Vec<StatefulRun>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let mut connection = self.pool.acquire().await?;
        let raw_ids = sqlx::query_scalar::<_, String>(
            "SELECT run.id FROM stateful_runs AS run
             JOIN stateful_run_threads AS thread ON thread.run_id = run.id
             WHERE thread.thread_id = ?
             ORDER BY run.created_at_ms DESC, run.id DESC LIMIT ?",
        )
        .bind(thread_id)
        .bind(i64::from(max_results))
        .fetch_all(&mut *connection)
        .await?;
        let mut runs = Vec::with_capacity(raw_ids.len());
        for raw_id in raw_ids {
            let id = StatefulRunId::parse(raw_id)?;
            runs.push(
                load_run(&mut connection, &id)
                    .await?
                    .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?,
            );
        }
        Ok(runs)
    }

    /// The project's most recently updated run, whatever its status.
    pub async fn latest_project_run(
        &self,
        project_id: &str,
    ) -> Result<Option<StatefulRun>, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        let Some(raw_id) = sqlx::query_scalar::<_, String>(
            "SELECT id FROM stateful_runs WHERE project_id = ?
             ORDER BY updated_at_ms DESC, id DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&mut *connection)
        .await?
        else {
            return Ok(None);
        };
        load_run(&mut connection, &StatefulRunId::parse(raw_id)?).await
    }
}

#[cfg(test)]
#[path = "run_history_storage_tests.rs"]
mod tests;
