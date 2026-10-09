//! The acceptance request: the run goal followed by the input of every applied steering
//! instruction. Applied steering is binding scope, so its sentences owe coverage exactly
//! like the goal's. Parts are appended in the order the ledger first sees them applied and
//! never move, so a request span recorded earlier keeps quoting the same text.

use std::ops::Range;

use sqlx::SqliteConnection;

use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::acceptance_storage::ensure_ledger;
use crate::storage::load_run;

pub(crate) struct AcceptanceRequest {
    pub(crate) text: String,
    /// Byte range of each applied steering input inside `text`, by steering ID.
    pub(crate) parts: Vec<(String, Range<usize>)>,
}

impl StatefulRunStore {
    /// The run's acceptance request text: user criteria quote it (`requestQuote`) and every
    /// sentence of it must be covered before completion.
    pub async fn acceptance_request(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<String, StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        let request = acceptance_request(&mut transaction, &run).await?;
        transaction.commit().await?;
        Ok(request.text)
    }
}

/// Appends newly applied steering to the run's request parts and returns the request. Must
/// run inside a write transaction.
pub(crate) async fn acceptance_request(
    connection: &mut SqliteConnection,
    run: &StatefulRun,
) -> Result<AcceptanceRequest, StatefulRunStoreError> {
    ensure_ledger(connection, &run.id).await?;
    let unrecorded = sqlx::query_scalar::<_, String>(
        "SELECT id FROM stateful_steering
         WHERE run_id = ? AND status = 'applied' AND id NOT IN (
             SELECT steering_id FROM stateful_acceptance_request_parts WHERE run_id = ?
         ) ORDER BY resulting_strategy_revision, created_at_ms, id",
    )
    .bind(run.id.as_str())
    .bind(run.id.as_str())
    .fetch_all(&mut *connection)
    .await?;
    for steering_id in unrecorded {
        sqlx::query(
            "INSERT INTO stateful_acceptance_request_parts (run_id, part, steering_id)
             SELECT ?, COALESCE(MAX(part), 0) + 1, ?
             FROM stateful_acceptance_request_parts WHERE run_id = ?",
        )
        .bind(run.id.as_str())
        .bind(&steering_id)
        .bind(run.id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    let inputs = sqlx::query_as::<_, (String, String)>(
        "SELECT parts.steering_id, steering.input
         FROM stateful_acceptance_request_parts AS parts
         JOIN stateful_steering AS steering ON steering.id = parts.steering_id
         WHERE parts.run_id = ? ORDER BY parts.part",
    )
    .bind(run.id.as_str())
    .fetch_all(&mut *connection)
    .await?;
    let mut text = run.value.goal.clone();
    let mut parts = Vec::with_capacity(inputs.len());
    for (steering_id, input) in inputs {
        text.push('\n');
        let start = text.len();
        text.push_str(&input);
        parts.push((steering_id, start..text.len()));
    }
    Ok(AcceptanceRequest { text, parts })
}
