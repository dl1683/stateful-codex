//! Inactive project-bound request materialization; legacy callers are unchanged.

use std::ops::Range;

use codex_project_store::ProjectWriteError;
use codex_project_store::ProjectWriteTransaction;
use thiserror::Error;

use crate::StatefulRunId;
use crate::StatefulRunStoreError;

const RUN: &str = "SELECT CASE WHEN typeof(id)='text' AND octet_length(id) BETWEEN 1 AND 128
    AND typeof(project_id)='text' AND project_id=? AND typeof(goal)='text' THEN 1 ELSE 0 END,
    octet_length(id)+octet_length(project_id)+octet_length(goal)
    FROM stateful_runs WHERE id=?";
const STEERING: &str = "SELECT CASE WHEN typeof(id)='text' AND octet_length(id) BETWEEN 1 AND 128
    AND typeof(run_id)='text' AND run_id=? AND typeof(project_id)='text' AND project_id=?
    AND typeof(input)='text' AND typeof(status)='text'
    AND status IN ('submitted','acknowledged','applied','rejected')
    AND typeof(revision)='integer' AND revision>0 AND typeof(created_at_ms)='integer'
    AND (resulting_strategy_revision IS NULL OR (typeof(resulting_strategy_revision)='integer'
        AND resulting_strategy_revision>=0))
    AND (status<>'applied' OR resulting_strategy_revision IS NOT NULL) THEN 1 ELSE 0 END,
    octet_length(id)+octet_length(run_id)+octet_length(project_id)+octet_length(status)
        +octet_length(revision)+COALESCE(octet_length(resulting_strategy_revision),0)
        +octet_length(created_at_ms)
        +CASE WHEN status='applied' THEN octet_length(input)+1 ELSE 0 END,
    CASE WHEN status='applied' AND NOT EXISTS (
        SELECT 1 FROM stateful_acceptance_request_parts p WHERE p.run_id=s.run_id AND p.steering_id=s.id)
        THEN octet_length(id)+octet_length(run_id) ELSE NULL END
    FROM stateful_steering s WHERE run_id=? LIMIT 129";
const PARTS: &str = "SELECT CASE WHEN typeof(p.part)='integer' AND p.part BETWEEN 1 AND 128
    AND typeof(p.run_id)='text' AND p.run_id=? AND typeof(p.steering_id)='text'
    AND octet_length(p.steering_id) BETWEEN 1 AND 128 AND s.id IS NOT NULL
    AND s.run_id=p.run_id AND s.project_id=? AND s.status='applied' THEN p.part END,
    octet_length(p.run_id)+octet_length(p.steering_id)+octet_length(p.part)
    FROM stateful_acceptance_request_parts p LEFT JOIN stateful_steering s ON s.id=p.steering_id
    WHERE p.run_id=? LIMIT 129";

#[derive(Debug, Error)]
pub enum ProjectRequestError {
    #[error(transparent)]
    Writer(#[from] ProjectWriteError),
    #[error(transparent)]
    Runtime(#[from] StatefulRunStoreError),
    #[error("project request has an invalid or foreign run, steering or part binding")]
    InvalidBinding,
    #[error("project request exceeds the complete materialization budget")]
    Limit,
}

impl From<sqlx::Error> for ProjectRequestError {
    fn from(error: sqlx::Error) -> Self {
        Self::Runtime(error.into())
    }
}

/// Provisional request text and UTF-8 byte ranges keyed by steering ID. This is
/// data, not a saved receipt, source verification or acceptance certificate.
#[derive(Debug, Eq, PartialEq)]
pub struct ProjectAcceptanceRequest {
    pub text: String,
    pub parts: Vec<(String, Range<usize>)>,
}

/// Materialize the admitted project's durable goal and applied steering under
/// one writer reservation. Terminal runs are readable too. Requires the actual
/// compiled runtime domain; never accepts a caller's substitute goal/project.
///
/// Returns the live guard WITHOUT committing. Text/parts are provisional until
/// the caller awaits commit, which also commits prior domain writes. Error or
/// cancellation consumes/drops the guard, queues rollback and makes its owner
/// close-only: await close before reopening/reconciling uncertain durability.
/// Explicit rollback after success is valid. Never hold over tools or inference.
///
/// Refuses >128 all-status steering rows, >128 final parts, final ordinals >128
/// or >65,536 total request/metadata bytes. Valid legacy excess is refused only
/// here. Bounds exclude SQLite internals and trusted compiled-schema validation.
/// No native routing, terminal transition or old-client exclusion is activated.
pub async fn materialize_project_request<'a>(
    mut transaction: ProjectWriteTransaction<'a>,
    run_id: &StatefulRunId,
) -> Result<(ProjectWriteTransaction<'a>, ProjectAcceptanceRequest), ProjectRequestError> {
    transaction.require_domain(&crate::project_store_domain().map_err(ProjectWriteError::from)?)?;
    if run_id.as_str().len() > 128 {
        return Err(ProjectRequestError::Limit);
    }
    let project = transaction.identity().project_uuid.to_string();
    let (valid, mut bytes): (i64, i64) = sqlx::query_as(RUN)
        .bind(&project)
        .bind(run_id.as_str())
        .fetch_optional(transaction.connection())
        .await?
        .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
    if valid != 1 {
        return Err(ProjectRequestError::InvalidBinding);
    }
    let steering: Vec<(i64, i64, Option<i64>)> = sqlx::query_as(STEERING)
        .bind(run_id.as_str())
        .bind(&project)
        .bind(run_id.as_str())
        .fetch_all(transaction.connection())
        .await?;
    let parts: Vec<(Option<i64>, i64)> = sqlx::query_as(PARTS)
        .bind(run_id.as_str())
        .bind(&project)
        .bind(run_id.as_str())
        .fetch_all(transaction.connection())
        .await?;
    if steering.len() > 128 || parts.len() > 128 {
        return Err(ProjectRequestError::Limit);
    }
    // L=octet_length, D=decimal integer length (NULL=0). Budget =
    // L(run.id,project_id,goal) + SUM(all candidates: L(id,run_id,project_id,status)
    // + D(revision,resulting_strategy_revision,created_at_ms)) + SUM(applied: L(input)+1 LF)
    // + SUM(FINAL parts: L(run_id,steering_id)+D(part)). Repeated appearances
    // are charged, including every NEW part and its assigned sparse-tail ordinal.
    // No other durable variable-length field is loaded by this operation.
    let mut new_count = 0;
    for (valid, size, new_part_bytes) in steering {
        if valid != 1 {
            return Err(ProjectRequestError::InvalidBinding);
        }
        bytes = bytes.checked_add(size).ok_or(ProjectRequestError::Limit)?;
        if let Some(size) = new_part_bytes {
            new_count += 1;
            bytes = bytes.checked_add(size).ok_or(ProjectRequestError::Limit)?;
        }
    }
    let mut last = 0;
    for (part, size) in &parts {
        last = last.max(part.ok_or(ProjectRequestError::InvalidBinding)?);
        bytes = bytes.checked_add(*size).ok_or(ProjectRequestError::Limit)?;
    }
    if parts.len() + new_count > 128
        || last + i64::try_from(new_count).map_err(|_| ProjectRequestError::Limit)? > 128
    {
        return Err(ProjectRequestError::Limit);
    }
    for offset in 1..=new_count {
        let ordinal = last + i64::try_from(offset).map_err(|_| ProjectRequestError::Limit)?;
        bytes = bytes
            .checked_add(
                i64::try_from(ordinal.to_string().len()).map_err(|_| ProjectRequestError::Limit)?,
            )
            .ok_or(ProjectRequestError::Limit)?;
    }
    if !(0..=65_536).contains(&bytes) {
        return Err(ProjectRequestError::Limit);
    }
    let goal: String = sqlx::query_scalar("SELECT goal FROM stateful_runs WHERE id=?")
        .bind(run_id.as_str())
        .fetch_one(transaction.connection())
        .await?;
    let request =
        crate::acceptance_request::acceptance_request(transaction.connection(), run_id, &goal)
            .await?;
    Ok((
        transaction,
        ProjectAcceptanceRequest {
            text: request.text,
            parts: request.parts,
        },
    ))
}

#[cfg(test)]
#[path = "project_request_tests.rs"]
mod tests;
