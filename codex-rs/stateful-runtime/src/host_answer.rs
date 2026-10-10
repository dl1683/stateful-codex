//! The "answered, unverified" terminal outcome of an Autonomous run.
//!
//! When the agent ends a turn of a running Autonomous run with a final answer and declares it
//! ready, the host ends the run as [`StatefulRunStatus::Answered`]: a truthful label for "the
//! agent answered; the host verified nothing". It is not Completed and satisfies no acceptance
//! criterion, so it needs no observation of what the turn did, and any action the run took
//! stays unverified. The exact answer and its basis are kept in the run's answer record
//! (migration 0013, whose table is reused unchanged); the run result is the answer text
//! without its outcome block.
//!
//! The transaction rechecks the run inside `BEGIN IMMEDIATE`. The caller's `authorize`
//! callback runs after every check, immediately before COMMIT; a refusal (an abort of the
//! answering turn won) rolls everything back.

use serde::Serialize;
use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::WorkflowMode;
use crate::run::MAX_RESULT_BYTES;
use crate::storage::load_run;
use crate::storage::unix_timestamp_millis;

/// The basis recorded with every answered run.
pub const ANSWERED_UNVERIFIED_BASIS: &str = "Answered, unverified: the run ended with the agent's final answer. The host verified neither the answer nor any work the run did, and no acceptance criterion was checked.";

/// A request to end a run with a turn's final answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnsweredRunCommit {
    pub run_id: StatefulRunId,
    pub thread_id: String,
    pub turn_id: String,
    /// The final assistant message exactly as the user received it, outcome block included.
    pub answer: String,
    /// The answer text without its outcome block; becomes the run result.
    pub result: String,
}

/// What ending a run with an answer did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnsweredRunOutcome {
    /// The run is Answered with this turn's answer (now, or already by the same turn).
    Answered(Box<StatefulRun>),
    /// The run is not a running Autonomous run bound to the thread; nothing changed.
    NotEligible,
    /// The answer cannot end the run, for the stated reason; nothing changed.
    Refused(String),
    /// The caller refused commit authorization (an abort won); nothing changed.
    NotAuthorized,
}

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

/// The published shape of an answer record; the size bound is measured on its encoding.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishedAnswer<'a> {
    turn_id: &'a str,
    answer: &'a str,
    basis: &'a str,
    committed_at: i64,
}

/// The bytes the answer record of `answer` takes once serialized, escapes and envelope
/// included (with the widest possible timestamp).
pub fn serialized_answer_bytes(turn_id: &str, answer: &str) -> usize {
    serde_json::to_vec(&PublishedAnswer {
        turn_id,
        answer,
        basis: ANSWERED_UNVERIFIED_BASIS,
        committed_at: i64::MIN,
    })
    .map_or(usize::MAX, |encoded| encoded.len())
}

impl StatefulRunStore {
    /// Ends `commit.run_id` as answered, unverified, if it is still a running Autonomous run
    /// bound to the answering thread and `authorize` permits the commit.
    pub async fn end_run_answered(
        &self,
        commit: &AnsweredRunCommit,
        authorize: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<AnsweredRunOutcome, StatefulRunStoreError> {
        if let Some(reason) = answer_violation(commit) {
            return Ok(AnsweredRunOutcome::Refused(reason));
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_host_answer(&mut transaction, &commit.run_id).await? {
            let run = load_run(&mut transaction, &commit.run_id)
                .await?
                .ok_or_else(|| StatefulRunStoreError::RunNotFound(commit.run_id.to_string()))?;
            transaction.commit().await?;
            return Ok(
                if existing.turn_id == commit.turn_id && run.status == StatefulRunStatus::Answered {
                    AnsweredRunOutcome::Answered(Box::new(run))
                } else {
                    AnsweredRunOutcome::NotEligible
                },
            );
        }
        let Some(run) = load_run(&mut transaction, &commit.run_id).await? else {
            return Ok(AnsweredRunOutcome::NotEligible);
        };
        if run.value.mode != WorkflowMode::Autonomous
            || run.status != StatefulRunStatus::Running
            || !run.value.thread_ids.contains(&commit.thread_id)
        {
            return Ok(AnsweredRunOutcome::NotEligible);
        }
        let unresolved_steering = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM stateful_steering
             WHERE run_id = ? AND status IN ('submitted', 'acknowledged')",
        )
        .bind(commit.run_id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        if unresolved_steering != 0 {
            return Ok(AnsweredRunOutcome::Refused(
                "steering for this run is still waiting to be applied".to_string(),
            ));
        }
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO stateful_host_answers (
                run_id, thread_id, turn_id, answer, basis, committed_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(commit.run_id.as_str())
        .bind(&commit.thread_id)
        .bind(&commit.turn_id)
        .bind(&commit.answer)
        .bind(ANSWERED_UNVERIFIED_BASIS)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        // Stored as `completed` together with the record above; it reads back as `answered`.
        let rows = sqlx::query(
            "UPDATE stateful_runs
             SET status = 'completed', result = ?, revision = revision + 1, updated_at_ms = ?
             WHERE id = ? AND status = 'running'",
        )
        .bind(&commit.result)
        .bind(now)
        .bind(commit.run_id.as_str())
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if rows != 1 {
            return Err(StatefulRunStoreError::ConcurrentMutation);
        }
        let run = load_run(&mut transaction, &commit.run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(commit.run_id.to_string()))?;
        if !authorize() {
            // Dropping the transaction rolls it back.
            return Ok(AnsweredRunOutcome::NotAuthorized);
        }
        transaction.commit().await?;
        Ok(AnsweredRunOutcome::Answered(Box::new(run)))
    }

    /// The answer record of the run, if it has one.
    pub async fn host_answer(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<Option<HostAnswerRecord>, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_host_answer(&mut connection, run_id).await
    }
}

/// Why `commit` cannot be stored whole, if it cannot.
fn answer_violation(commit: &AnsweredRunCommit) -> Option<String> {
    if commit.result.trim().is_empty()
        || !commit.answer.contains(&commit.result)
        || commit.answer.contains('\0')
    {
        return Some("the answer is empty or malformed".to_string());
    }
    let bytes = serialized_answer_bytes(&commit.turn_id, &commit.answer);
    (bytes > MAX_RESULT_BYTES).then(|| {
        format!(
            "the answer takes {bytes} bytes once recorded, over the {MAX_RESULT_BYTES}-byte limit"
        )
    })
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

#[cfg(test)]
#[path = "host_answer_tests.rs"]
mod tests;
