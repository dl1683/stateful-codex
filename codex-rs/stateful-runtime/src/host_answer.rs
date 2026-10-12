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
//! When the commit lists unverified items, the run ends Partial instead: the same stored
//! status, the [`PARTIAL_UNVERIFIED_BASIS`], and a result that lists the items before the
//! answer text (see [`compose_partial`]).
//!
//! The transaction rechecks the run inside `BEGIN IMMEDIATE`. The caller's `authorize`
//! callback runs after every check, immediately before COMMIT; a refusal (an abort of the
//! answering turn won) rolls everything back.

use serde::Serialize;
use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::AcceptanceOrigin;
use crate::AcceptanceState;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::WorkflowMode;
use crate::acceptance_storage::load_ledger;
use crate::obligation_operations::latest_obligation_packet;
use crate::run::MAX_RESULT_BYTES;
use crate::storage::load_run;
use crate::storage::unix_timestamp_millis;

/// The basis recorded with every answered run.
pub const ANSWERED_UNVERIFIED_BASIS: &str = "Answered, unverified: the run ended with the agent's final answer. The host verified neither the answer nor any work the run did, and no acceptance criterion was checked.";

/// The basis recorded with every partial run. Its bytes are storage semantics: the published
/// outcome is derived from them, so changing them needs a migration.
pub const PARTIAL_UNVERIFIED_BASIS: &str = "Partial, not certified: the run ended with the agent's final answer while the listed items were open or not certified by this terminal answer path. Earlier host evidence stays in the acceptance ledger; this answer path checked none of it.";

/// The first line of a partial run's result; the listed items follow, one `- ` line each.
pub const PARTIAL_RESULT_HEADER: &str = "Partial result (not certified). Open or not certified:\n";
/// Longest criterion statement quoted in an unverified item.
const MAX_ITEM_STATEMENT_BYTES: usize = 120;

/// How an answered run ended, as published with its answer record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostAnswerOutcome {
    /// Nothing was listed as open or not certified.
    Answered,
    /// The result lists open or not-certified items before the answer.
    Partial,
}

impl HostAnswerOutcome {
    /// The basis recorded for this outcome.
    pub fn basis(self) -> &'static str {
        match self {
            Self::Answered => ANSWERED_UNVERIFIED_BASIS,
            Self::Partial => PARTIAL_UNVERIFIED_BASIS,
        }
    }
}

/// A request to end a run with a turn's final answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnsweredRunCommit {
    pub run_id: StatefulRunId,
    pub thread_id: String,
    pub turn_id: String,
    /// The final assistant message exactly as the user received it, outcome block included.
    pub answer: String,
    /// The answer text without its outcome block; becomes the run result (after the listed
    /// items, when the run ends Partial).
    pub result: String,
    /// Open issues the answer's outcome block declared, in order; any makes the run Partial.
    pub trailer_issues: Vec<String>,
}

/// What ending a run with an answer did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnsweredRunOutcome {
    /// The run is Answered with this turn's answer (now, or already by the same turn), with
    /// the answer record: the one just inserted, or the existing one on a same-turn retry.
    Answered(Box<StatefulRun>, Box<HostAnswerRecord>),
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

impl HostAnswerRecord {
    /// Partial exactly when the record carries the partial basis.
    pub fn outcome(&self) -> HostAnswerOutcome {
        if self.basis == PARTIAL_UNVERIFIED_BASIS {
            HostAnswerOutcome::Partial
        } else {
            HostAnswerOutcome::Answered
        }
    }
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
    outcome: HostAnswerOutcome,
}

/// The bytes the answer record of `answer` takes once serialized with `outcome` and its basis,
/// escapes and envelope included (with the widest possible timestamp).
pub fn serialized_answer_bytes(turn_id: &str, answer: &str, outcome: HostAnswerOutcome) -> usize {
    serde_json::to_vec(&PublishedAnswer {
        turn_id,
        answer,
        basis: outcome.basis(),
        committed_at: i64::MIN,
        outcome,
    })
    .map_or(usize::MAX, |encoded| encoded.len())
}

impl StatefulRunStore {
    /// Ends `commit.run_id` as answered, unverified (or partial, when items are listed), if it
    /// is still a running Autonomous run bound to the answering thread and `authorize` permits
    /// the commit.
    pub async fn end_run_answered(
        &self,
        commit: &AnsweredRunCommit,
        authorize: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<AnsweredRunOutcome, StatefulRunStoreError> {
        let requested = if commit.trailer_issues.is_empty() {
            HostAnswerOutcome::Answered
        } else {
            HostAnswerOutcome::Partial
        };
        if let Some(reason) = answer_violation(commit, requested) {
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
                    AnsweredRunOutcome::Answered(Box::new(run), Box::new(existing))
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
        let (outcome, result) = if commit.trailer_issues.is_empty() {
            (HostAnswerOutcome::Answered, commit.result.clone())
        } else {
            let Some(result) = compose_partial(&commit.trailer_issues, &commit.result) else {
                return Ok(AnsweredRunOutcome::Refused(format!(
                    "the answer and its list of open or not-certified items do not fit the {MAX_RESULT_BYTES}-byte result limit"
                )));
            };
            (HostAnswerOutcome::Partial, result)
        };
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
        .bind(outcome.basis())
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        // Stored as `completed` together with the record above; it reads back as `answered`.
        let rows = sqlx::query(
            "UPDATE stateful_runs
             SET status = 'completed', result = ?, revision = revision + 1, updated_at_ms = ?
             WHERE id = ? AND status = 'running'",
        )
        .bind(&result)
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
        let record = HostAnswerRecord {
            run_id: commit.run_id.clone(),
            thread_id: commit.thread_id.clone(),
            turn_id: commit.turn_id.clone(),
            answer: commit.answer.clone(),
            basis: outcome.basis().to_string(),
            committed_at_ms: now,
        };
        Ok(AnsweredRunOutcome::Answered(
            Box::new(run),
            Box::new(record),
        ))
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

/// Why `commit` cannot be stored whole with `outcome`, if it cannot.
fn answer_violation(commit: &AnsweredRunCommit, outcome: HostAnswerOutcome) -> Option<String> {
    if commit.result.trim().is_empty()
        || !commit.answer.contains(&commit.result)
        || commit.answer.contains('\0')
    {
        return Some("the answer is empty or malformed".to_string());
    }
    let bytes = serialized_answer_bytes(&commit.turn_id, &commit.answer, outcome);
    (bytes > MAX_RESULT_BYTES).then(|| {
        format!(
            "the answer takes {bytes} bytes once recorded, over the {MAX_RESULT_BYTES}-byte limit"
        )
    })
}

/// The result of a partial run: [`PARTIAL_RESULT_HEADER`], one `- item` line per item, the
/// omission marker when some did not fit, a blank line, then `body`. Room for the marker of
/// every item is reserved before any item is added; items are added in order while they fit.
/// `None` when even the header, the reserved marker and the body exceed the result limit.
pub(crate) fn compose_partial(items: &[String], body: &str) -> Option<String> {
    let reserved = PARTIAL_RESULT_HEADER.len() + partial_marker(items.len()).len() + 2 + body.len();
    if reserved > MAX_RESULT_BYTES {
        return None;
    }
    let mut lines = String::new();
    let mut kept = 0;
    for item in items {
        // One line per item, so the list stays parseable; the byte length is unchanged.
        let line = format!("- {}\n", item.replace(['\n', '\r'], " "));
        if reserved + lines.len() + line.len() > MAX_RESULT_BYTES {
            break;
        }
        lines.push_str(&line);
        kept += 1;
    }
    let marker = if kept < items.len() {
        partial_marker(items.len() - kept)
    } else {
        String::new()
    };
    Some(format!("{PARTIAL_RESULT_HEADER}{lines}{marker}\n{body}"))
}

/// The line that ends the listed items of a partial result when `omitted` of them did not fit.
pub fn partial_marker(omitted: usize) -> String {
    format!("- +{omitted} more (read the acceptance ledger and obligation)\n")
}

/// The items a terminal answer leaves open or not certified, read on the transaction's
/// connection: each Active, required criterion (User, Derived, or an accepted Omission), then
/// every blocker of the latest obligation. Untouched omission proposals are not listed.
// The activation step wires this into `end_run_answered`; until then only tests call it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) async fn unverified_items(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<Vec<String>, StatefulRunStoreError> {
    let ledger = load_ledger(connection, run_id).await?;
    let mut items = Vec::new();
    for criterion in &ledger.criteria {
        let declared = match criterion.origin {
            // An Omission criterion is Active only once accepted.
            AcceptanceOrigin::User | AcceptanceOrigin::Derived | AcceptanceOrigin::Omission => {
                criterion.state == AcceptanceState::Active && criterion.required
            }
        };
        if declared {
            let statement = &criterion.statement;
            let statement = &statement[..statement.floor_char_boundary(MAX_ITEM_STATEMENT_BYTES)];
            items.push(format!(
                "{} ({statement}): not certified by this terminal answer path",
                criterion.alias()
            ));
        }
    }
    if let Some((_, packet)) = latest_obligation_packet(connection, run_id).await? {
        items.extend(packet.blockers);
    }
    Ok(items)
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
