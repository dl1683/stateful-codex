//! Read-only access to the answer records of migration 0013.
//!
//! Each record keeps the exact final answer a run ended with and the basis stated with it.

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;

/// The durable record of the answer a run ended with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostAnswerRecord {
    pub run_id: StatefulRunId,
    pub thread_id: String,
    pub turn_id: String,
    pub answer: String,
    pub basis: String,
    pub committed_at_ms: i64,
}

#[derive(FromRow)]
struct StoredHostAnswer {
    run_id: String,
    thread_id: String,
    turn_id: String,
    answer: String,
    basis: String,
    committed_at_ms: i64,
}

impl StatefulRunStore {
    /// The answer record of the run, if it has one.
    pub async fn host_answer(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<Option<HostAnswerRecord>, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_host_answer(&mut connection, run_id).await
    }
}

async fn load_host_answer(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<Option<HostAnswerRecord>, StatefulRunStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredHostAnswer>(
        "SELECT run_id, thread_id, turn_id, answer, basis, committed_at_ms
         FROM stateful_host_answers WHERE run_id = ?",
    )
    .bind(run_id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(HostAnswerRecord {
        run_id: StatefulRunId::parse(stored.run_id)?,
        thread_id: stored.thread_id,
        turn_id: stored.turn_id,
        answer: stored.answer,
        basis: stored.basis,
        committed_at_ms: stored.committed_at_ms,
    }))
}
