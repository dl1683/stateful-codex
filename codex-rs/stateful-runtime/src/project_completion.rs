use codex_project_store::DomainManifest;
use codex_project_store::ProjectWriteError;
use codex_project_store::ProjectWriteTransaction;
use sqlx::SqliteConnection;
use thiserror::Error;

use crate::AcceptanceCommit;
use crate::NewObligation;
use crate::StatefulObligation;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStoreError;
use crate::StatefulRunUpdate;
use crate::storage;

/// Trusted runtime inputs; callers must first validate knowledge, eligibility and
/// source bindings in the same transaction. Native routing remains inactive.
pub struct ProjectRunCompletion<'a> {
    pub run_id: &'a StatefulRunId,
    pub update: StatefulRunUpdate,
    pub acceptance: &'a AcceptanceCommit,
    pub final_obligation: Option<(String, NewObligation)>,
}

#[derive(Debug, Error)]
pub enum ProjectRunCompletionError {
    #[error(transparent)]
    Writer(#[from] ProjectWriteError),
    #[error(transparent)]
    Runtime(#[from] StatefulRunStoreError),
    #[error("run or its dependent rows belong to another project")]
    ProjectMismatch,
}

/// Consume and commit the guard with prior domain writes. Failure/cancellation
/// drops it and makes its writer close-only; no guard survives runtime errors.
/// Enforces runtime acceptance, not knowledge policy, whole-store or native admission.
pub async fn commit_project_run(
    mut transaction: ProjectWriteTransaction<'_>,
    request: ProjectRunCompletion<'_>,
) -> Result<(StatefulRun, Option<StatefulObligation>), ProjectRunCompletionError> {
    transaction.require_domain(&DomainManifest {
        domain: "stateful-runtime",
        migrations: &storage::MIGRATOR,
    })?;
    validate_request(
        &request.update,
        request.final_obligation.as_ref(),
        request.run_id,
    )?;
    let project = transaction.identity().project_uuid.to_string();
    let current = storage::load_run(transaction.connection(), request.run_id)
        .await?
        .ok_or_else(|| StatefulRunStoreError::RunNotFound(request.run_id.to_string()))?;
    let foreign: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM stateful_obligations WHERE run_id=? AND project_id<>?
         UNION ALL SELECT 1 FROM stateful_steering WHERE run_id=? AND project_id<>?
         UNION ALL SELECT 1 FROM stateful_turn_measurements WHERE run_id=? AND project_id<>?)",
    )
    .bind(request.run_id.as_str())
    .bind(&project)
    .bind(request.run_id.as_str())
    .bind(&project)
    .bind(request.run_id.as_str())
    .bind(&project)
    .fetch_one(transaction.connection())
    .await
    .map_err(StatefulRunStoreError::from)?;
    if current.value.project_id != project
        || foreign
        || request
            .final_obligation
            .as_ref()
            .is_some_and(|(_, value)| value.project_id != project)
    {
        return Err(ProjectRunCompletionError::ProjectMismatch);
    }
    let result = complete_on(
        transaction.connection(),
        request.run_id,
        request.update,
        request.acceptance,
        request.final_obligation,
    )
    .await?;
    transaction.commit().await?;
    Ok(result)
}

pub(crate) fn validate_request(
    update: &StatefulRunUpdate,
    obligation: Option<&(String, NewObligation)>,
    id: &StatefulRunId,
) -> Result<(), StatefulRunStoreError> {
    update.validate()?;
    if update.status != StatefulRunStatus::Completed {
        return Err(StatefulRunStoreError::CompletionStatusRequired);
    }
    if let Some((obligation_id, obligation)) = obligation {
        storage::validate_record_id(obligation_id)?;
        obligation.validate()?;
        if obligation.run_id != *id {
            return Err(StatefulRunStoreError::ObligationRunMismatch);
        }
        if !obligation.packet.blockers.is_empty() {
            return Err(StatefulRunStoreError::AcceptanceGate(
                "the final obligation declares open blockers".to_string(),
            ));
        }
    }
    Ok(())
}

pub(crate) async fn complete_on(
    connection: &mut SqliteConnection,
    id: &StatefulRunId,
    update: StatefulRunUpdate,
    acceptance: &AcceptanceCommit,
    obligation: Option<(String, NewObligation)>,
) -> Result<(StatefulRun, Option<StatefulObligation>), StatefulRunStoreError> {
    let run = storage::update_run_in_transaction(connection, id, update, Some(acceptance)).await?;
    let obligation = match obligation {
        Some((obligation_id, obligation)) => Some(
            storage::append_obligation_in_transaction(connection, obligation_id, obligation)
                .await?,
        ),
        None => None,
    };
    Ok((run, obligation))
}

#[cfg(test)]
#[path = "project_completion_tests.rs"]
mod tests;
