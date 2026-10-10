//! The host-ended answer: the one terminal path that completes an Autonomous run with its
//! first answering task's final answer and no acceptance ledger.
//!
//! The caller (the host's finalizer) holds a reservation for exactly one answering task and
//! has observed that task end with a final answer, no tool call and no auxiliary work. This
//! module re-checks, inside the terminal transaction, everything durable that could make such
//! a completion false: the run must still be the same Running Autonomous run bound to that one
//! thread, never continued, with an untouched acceptance ledger, no obligation, no steering
//! other than rejected steering, and no earlier host answer. The `authorize` callback is the
//! reservation's commit authorization; it runs after every check and immediately before the
//! COMMIT, and a refusal rolls the transaction back.
//!
//! Ordinary completion writers are unaffected: they still go through the acceptance gate.

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

/// The basis recorded with every host-ended answer.
pub const HOST_ANSWER_BASIS: &str = "Host-ended answer: the answering task observed no tool call and admitted no user command or executable hook. The answer is judged by the user; it is not host-verified. Host housekeeping and independent external activity are outside this observation scope, and a provider that runs a hosted tool without emitting any observable item cannot be certified.";

/// A request to end a run with its first answering task's final answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostAnswerCommit {
    pub run_id: StatefulRunId,
    /// The run revision the finalizer validated.
    pub expected_revision: u64,
    pub thread_id: String,
    pub turn_id: String,
    /// The final answer exactly as the user received it.
    pub answer: String,
}

/// The durable record of a host-ended answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostAnswerRecord {
    pub run_id: StatefulRunId,
    pub thread_id: String,
    pub turn_id: String,
    pub answer: String,
    pub basis: String,
    pub committed_at_ms: i64,
}

/// What a host-ended answer commit did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAnswerOutcome {
    /// The run is Completed with this answer (now, or already by the same turn).
    Committed(Box<StatefulRun>),
    /// A durable fact makes this completion false; nothing changed.
    Refused(String),
    /// The reservation refused commit authorization (an abort won); nothing changed.
    NotAuthorized,
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
    /// Completes `commit.run_id` with the host-ended answer if every durable precondition
    /// still holds and `authorize` permits the commit.
    pub async fn complete_run_with_host_answer(
        &self,
        commit: &HostAnswerCommit,
        authorize: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<HostAnswerOutcome, StatefulRunStoreError> {
        if let Some(reason) = answer_budget_violation(&commit.answer) {
            return Ok(HostAnswerOutcome::Refused(reason));
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_host_answer(&mut transaction, &commit.run_id).await? {
            let run = load_run(&mut transaction, &commit.run_id)
                .await?
                .ok_or_else(|| StatefulRunStoreError::RunNotFound(commit.run_id.to_string()))?;
            transaction.commit().await?;
            return Ok(
                if existing.turn_id == commit.turn_id && existing.thread_id == commit.thread_id {
                    HostAnswerOutcome::Committed(Box::new(run))
                } else {
                    HostAnswerOutcome::Refused(
                        "another turn already ended this run with a host answer".to_string(),
                    )
                },
            );
        }
        if let Some(reason) = refusal(&mut transaction, commit).await? {
            return Ok(HostAnswerOutcome::Refused(reason));
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
        .bind(HOST_ANSWER_BASIS)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let expected_revision = i64::try_from(commit.expected_revision)
            .map_err(|_| StatefulRunStoreError::CountOverflow)?;
        let rows = sqlx::query(
            "UPDATE stateful_runs
             SET status = 'completed', result = ?, revision = revision + 1, updated_at_ms = ?
             WHERE id = ? AND revision = ? AND status = 'running'",
        )
        .bind(commit.answer.trim())
        .bind(now)
        .bind(commit.run_id.as_str())
        .bind(expected_revision)
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
            return Ok(HostAnswerOutcome::NotAuthorized);
        }
        transaction.commit().await?;
        Ok(HostAnswerOutcome::Committed(Box::new(run)))
    }

    /// The run's host-ended answer, if it has one.
    pub async fn host_answer(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<Option<HostAnswerRecord>, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_host_answer(&mut connection, run_id).await
    }
}

/// Why `answer` cannot be stored whole with its basis, if it cannot.
fn answer_budget_violation(answer: &str) -> Option<String> {
    if answer.trim().is_empty() || answer.contains('\0') {
        return Some("the final answer is empty or contains NUL".to_string());
    }
    let total = answer.len().saturating_add(HOST_ANSWER_BASIS.len());
    (total > MAX_RESULT_BYTES).then(|| {
        format!(
            "the final answer and its basis take {total} bytes, over the {MAX_RESULT_BYTES}-byte result limit"
        )
    })
}

/// The first durable fact that makes a host-ended answer false, if any.
async fn refusal(
    connection: &mut SqliteConnection,
    commit: &HostAnswerCommit,
) -> Result<Option<String>, StatefulRunStoreError> {
    let Some(run) = load_run(connection, &commit.run_id).await? else {
        return Err(StatefulRunStoreError::RunNotFound(
            commit.run_id.to_string(),
        ));
    };
    if run.revision != commit.expected_revision {
        return Ok(Some(format!(
            "the run changed (revision {} instead of {})",
            run.revision, commit.expected_revision
        )));
    }
    if run.value.mode != WorkflowMode::Autonomous || run.status != StatefulRunStatus::Running {
        return Ok(Some(
            "the run is no longer a Running Autonomous run".to_string(),
        ));
    }
    if run.value.thread_ids != [commit.thread_id.clone()] {
        return Ok(Some(
            "the run is not bound to exactly the answering thread".to_string(),
        ));
    }
    let run_id = commit.run_id.as_str();
    let durable_activity = sqlx::query_scalar::<_, i64>(
        "SELECT
            (SELECT COUNT(*) FROM stateful_run_continuations WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_run_leases WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_obligations WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_steering WHERE run_id = ?1 AND status <> 'rejected')
          + (SELECT COUNT(*) FROM stateful_acceptance_criteria WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_acceptance_evidence WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_acceptance_pending WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_acceptance_steering WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_acceptance_request_parts WHERE run_id = ?1)
          + (SELECT COUNT(*) FROM stateful_acceptance_ledgers WHERE run_id = ?1 AND (
                revision <> 0 OR workspace_generation <> 0 OR observed_executions <> 0
                OR stalled_completions <> 0 OR verification_attempt <> 0
                OR verification_owner IS NOT NULL OR side_effects <> 0
                OR exemption IS NOT NULL OR completion_attempts <> 0
            ))",
    )
    .bind(run_id)
    .fetch_one(&mut *connection)
    .await?;
    if durable_activity != 0 {
        return Ok(Some(
            "the run already has recorded work (a continuation, obligation, steering, or acceptance record)"
                .to_string(),
        ));
    }
    Ok(None)
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
