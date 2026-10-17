//! Inactive project-bound run creation, preserving the legacy creation policy.
use codex_project_store::ProjectWriteError;
use codex_project_store::ProjectWriteTransaction;
use sqlx::SqliteConnection;
use thiserror::Error;

use crate::NewStatefulRun;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStoreError;
use crate::WorkflowMode;
use crate::storage::load_run;
use crate::storage::mode_name;
use crate::storage::status_name;
use crate::storage::unix_timestamp_millis;

#[derive(Debug, Error)]
pub enum ProjectRunCreationError {
    #[error(transparent)]
    Writer(#[from] ProjectWriteError),
    #[error(transparent)]
    Runtime(#[from] StatefulRunStoreError),
    #[error("run does not belong to the admitted project")]
    ProjectMismatch,
    #[error("existing run exceeds the complete creation-replay read budget")]
    ReadLimit,
}

/// Create or replay a run and commit prior domain writes atomically. Requires the
/// actual compiled runtime domain and exact project UUID. Same-value replays
/// preserve the current run, including its terminal state, as in legacy creation.
/// This is trusted domain access, not foreign-agent authority or native routing.
///
/// Existing-run reads refuse >64 threads or >131,072 stored field bytes across
/// the full run projection and thread IDs/positions. Refusal/cancellation drops
/// the guard and leaves its owner close-only. Await close before reconciliation;
/// an uncertain COMMIT requires reconciling ALL prior writes after close/reopen.
/// Same-ID/value replay alone does not prove those other writes committed.
/// Never hold the reservation over commands, scans or inference.
pub async fn commit_project_run_creation(
    mut transaction: ProjectWriteTransaction<'_>,
    id: StatefulRunId,
    value: NewStatefulRun,
) -> Result<StatefulRun, ProjectRunCreationError> {
    transaction.require_domain(&crate::project_store_domain().map_err(ProjectWriteError::from)?)?;
    value.validate().map_err(StatefulRunStoreError::from)?;
    let project = transaction.identity().project_uuid.to_string();
    if value.project_id != project {
        return Err(ProjectRunCreationError::ProjectMismatch);
    }
    let metadata: Option<(bool, i64)> = sqlx::query_as(
        "SELECT typeof(project_id)='text' AND project_id=?,
         octet_length(id)+octet_length(project_id)+octet_length(goal)+octet_length(mode)
         +octet_length(status)+COALESCE(octet_length(strategy),0)+octet_length(strategy_revision)
         +COALESCE(octet_length(result),0)+octet_length(revision)+octet_length(created_at_ms)
         +octet_length(updated_at_ms)+octet_length(max_continuations)
         +octet_length(max_elapsed_seconds)+octet_length(continuations_used)
         FROM stateful_runs WHERE id=?",
    )
    .bind(&project)
    .bind(id.as_str())
    .fetch_optional(transaction.connection())
    .await
    .map_err(StatefulRunStoreError::from)?;
    if let Some((owned, bytes)) = metadata {
        if !owned {
            return Err(ProjectRunCreationError::ProjectMismatch);
        }
        let (thread_bytes, count): (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(bytes),0), COUNT(*) FROM (
             SELECT octet_length(thread_id)+octet_length(position) AS bytes
             FROM stateful_run_threads WHERE run_id=? LIMIT 65)",
        )
        .bind(id.as_str())
        .fetch_one(transaction.connection())
        .await
        .map_err(StatefulRunStoreError::from)?;
        if count > 64 || !(0..=131_072).contains(&bytes.saturating_add(thread_bytes)) {
            return Err(ProjectRunCreationError::ReadLimit);
        }
    }
    let run = create_on(transaction.connection(), id, value).await?;
    transaction.commit().await?;
    Ok(run)
}

pub(crate) async fn create_on(
    connection: &mut SqliteConnection,
    id: StatefulRunId,
    value: NewStatefulRun,
) -> Result<StatefulRun, StatefulRunStoreError> {
    if let Some(existing) = load_run(&mut *connection, &id).await? {
        if existing.value != value {
            return Err(StatefulRunStoreError::RunIdentityConflict(id.to_string()));
        }
        return Ok(existing);
    }
    let now = unix_timestamp_millis()?;
    let status = match value.mode {
        WorkflowMode::Socratic => StatefulRunStatus::Pending,
        WorkflowMode::Autonomous | WorkflowMode::Collaborative => StatefulRunStatus::Running,
    };
    sqlx::query(
        "INSERT INTO stateful_runs (
                id, project_id, goal, mode, status, strategy, strategy_revision,
                result, revision, created_at_ms, updated_at_ms, max_continuations,
                max_elapsed_seconds, continuations_used
             ) VALUES (?, ?, ?, ?, ?, NULL, 0, NULL, ?, ?, ?, ?, ?, 0)",
    )
    .bind(id.as_str())
    .bind(&value.project_id)
    .bind(&value.goal)
    .bind(mode_name(value.mode))
    .bind(status_name(status))
    .bind(/*value*/ 1_i64)
    .bind(now)
    .bind(now)
    .bind(i64::from(value.budget.max_continuations))
    .bind(i64::from(value.budget.max_elapsed_seconds))
    .execute(&mut *connection)
    .await?;
    for (position, thread_id) in value.thread_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO stateful_run_threads (run_id, position, thread_id)
                 VALUES (?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(i64::try_from(position).map_err(|_| StatefulRunStoreError::CountOverflow)?)
        .bind(thread_id)
        .execute(&mut *connection)
        .await?;
    }
    let run = load_run(&mut *connection, &id)
        .await?
        .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
    Ok(run)
}

#[cfg(test)]
#[path = "project_creation_tests.rs"]
mod tests;
