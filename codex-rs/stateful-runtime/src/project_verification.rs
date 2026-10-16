//! Inactive project-store lease operations, sharing the legacy verification policy.
use codex_project_store::DomainManifest;
use codex_project_store::ProjectWriteError;
use codex_project_store::ProjectWriteTransaction;
use thiserror::Error;

use crate::StatefulRunId;
use crate::StatefulRunStoreError;

#[derive(Debug, Error)]
pub enum ProjectVerificationError {
    #[error(transparent)]
    Writer(#[from] ProjectWriteError),
    #[error(transparent)]
    Runtime(#[from] StatefulRunStoreError),
    #[error("verification run does not belong to the admitted project")]
    ProjectMismatch,
}

/// Consume and commit the guard, including any prior domain writes, with a
/// project-bound verification lease. This preserves legacy same-owner takeover
/// and competing-owner expiry rules; a lease is not acceptance/checker evidence.
/// No native attachment or cross-database operation is performed.
///
/// Refusal/cancellation drops the guard and leaves its owner close-only. Await
/// close before reconciliation; cancellation during COMMIT has uncertain outcome.
pub async fn commit_project_verification_start(
    mut transaction: ProjectWriteTransaction<'_>,
    run_id: &StatefulRunId,
    owner: &str,
    lease_ms: u32,
) -> Result<u64, ProjectVerificationError> {
    require_run(&mut transaction, run_id).await?;
    crate::run::validate_source_id(owner).map_err(StatefulRunStoreError::from)?;
    let attempt = crate::verification::begin_on(transaction.connection(), run_id, owner, lease_ms).await?;
    transaction.commit().await?;
    Ok(attempt)
}

/// Consume and commit the guard, including prior domain writes, releasing only
/// this exact owner/attempt. A stale attempt is a no-op, never another lease's
/// release. Terminal runs may release their old lease, matching legacy policy.
/// Failure/cancellation is close-only; COMMIT acknowledgement can be uncertain.
pub async fn commit_project_verification_end(
    mut transaction: ProjectWriteTransaction<'_>,
    run_id: &StatefulRunId,
    owner: &str,
    attempt: u64,
) -> Result<(), ProjectVerificationError> {
    require_run(&mut transaction, run_id).await?;
    let attempt = i64::try_from(attempt).map_err(|_| StatefulRunStoreError::CountOverflow)?;
    crate::verification::end_on(transaction.connection(), run_id, owner, attempt).await?;
    transaction.commit().await?;
    Ok(())
}

async fn require_run(
    transaction: &mut ProjectWriteTransaction<'_>,
    run_id: &StatefulRunId,
) -> Result<(), ProjectVerificationError> {
    transaction.require_domain(&DomainManifest { domain: "stateful-runtime", migrations: &crate::storage::MIGRATOR })?;
    let project = transaction.identity().project_uuid.to_string();
    let owner: Option<String> = sqlx::query_scalar(
        "SELECT CASE WHEN typeof(project_id)='text' AND octet_length(project_id)=36
         THEN project_id END FROM stateful_runs WHERE id=?",
    )
    .bind(run_id.as_str())
    .fetch_optional(transaction.connection())
    .await
    .map_err(StatefulRunStoreError::from)?
    .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
    if owner.as_deref() != Some(project.as_str()) {
        return Err(ProjectVerificationError::ProjectMismatch);
    }
    Ok(())
}

#[cfg(test)]
#[path = "project_verification_tests.rs"]
mod tests;
