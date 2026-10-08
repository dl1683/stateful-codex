//! Host-owned check-plan admission and steering reconciliation. A plan is admitted only from
//! the ledger's own terms and the host's reading of the checker bytes, never from quoted user
//! text or a model's mention of a command; any change of scope, method, checker or
//! dependencies requires a new admission.

use sqlx::SqliteConnection;

use crate::AcceptanceCriterion;
use crate::AcceptanceState;
use crate::StatefulRunStoreError;
use crate::acceptance::AcceptanceError;
use crate::acceptance::MAX_REASON_BYTES;
use crate::acceptance::validate_bounded;
use crate::acceptance_changes::ChangeContext;
use crate::acceptance_changes::count;

/// The host admits a criterion's check plan only from the ledger's own terms: an active
/// criterion with a check command, an expected observation, pinned artifacts and checker
/// files, where the command itself names at least one checker file (so a command that runs
/// no checker, such as an echo, is refused) and the checker files were readable on the local
/// executor. The admission freezes the criterion revision and the checker bytes.
pub(crate) async fn admit_plan(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    criterion: &AcceptanceCriterion,
    checker_digest: Option<String>,
) -> Result<bool, StatefulRunStoreError> {
    let alias = criterion.alias();
    let refused =
        |message: String| -> StatefulRunStoreError { AcceptanceError::Refused(message).into() };
    if criterion.state != AcceptanceState::Active {
        return Err(refused(format!("{alias} is not active")));
    }
    let Some(command) = criterion.check_command.as_deref() else {
        return Err(refused(format!("{alias} has no checkCommand to admit")));
    };
    if criterion.checker.is_empty() || criterion.artifacts.is_empty() {
        return Err(refused(format!(
            "{alias} needs checker files (the tests or scripts the command runs) and pinned artifacts before its plan can be admitted"
        )));
    }
    if !names_checker(command, &criterion.checker) {
        return Err(refused(format!(
            "`{command}` names none of {alias}'s checker files, so it is not a check of them; a command such as an echo cannot be admitted"
        )));
    }
    let Some(checker_digest) = checker_digest else {
        return Err(refused(format!(
            "{alias}'s checker files could not be read on the local executor, so their bytes cannot be frozen"
        )));
    };
    if criterion.plan.as_ref().is_some_and(|plan| {
        plan.criterion_revision == criterion.revision && plan.checker_digest == checker_digest
    }) {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET plan_criterion_revision = ?, plan_checker_digest = ?, plan_ledger_revision = ?,
             ledger_revision = ?, updated_at_ms = ?
         WHERE run_id = ? AND ordinal = ?",
    )
    .bind(count(criterion.revision)?)
    .bind(&checker_digest)
    .bind(count(context.ledger_revision)?)
    .bind(count(context.ledger_revision)?)
    .bind(context.now)
    .bind(context.run.id.as_str())
    .bind(i64::from(criterion.ordinal))
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

/// Whether a command names one of the checker files by path or file name.
fn names_checker(command: &str, checker: &[String]) -> bool {
    checker.iter().any(|path| {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        command.contains(path.as_str()) || (!name.is_empty() && command.contains(name))
    })
}

/// Records how an applied steering instruction affects acceptance. The scope may have
/// changed, so every admitted plan is withdrawn and earlier evidence becomes stale.
pub(crate) async fn reconcile_steering(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    steering_id: &str,
    reason: &str,
) -> Result<bool, StatefulRunStoreError> {
    validate_bounded(reason, MAX_REASON_BYTES)?;
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM stateful_steering WHERE id = ? AND run_id = ?",
    )
    .bind(steering_id)
    .bind(context.run.id.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    if status.as_deref() != Some("applied") {
        return Err(AcceptanceError::Refused(format!(
            "steering {steering_id} is not an applied instruction of this run"
        ))
        .into());
    }
    if context
        .ledger
        .reconciled_steering
        .iter()
        .any(|reconciled| reconciled.steering_id == steering_id)
    {
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO stateful_acceptance_steering (run_id, steering_id, reason, reconciled_at_ms)
         VALUES (?, ?, ?, ?)",
    )
    .bind(context.run.id.as_str())
    .bind(steering_id)
    .bind(reason)
    .bind(context.now)
    .execute(&mut *connection)
    .await?;
    // An empty ledger holds no admission or evidence to withdraw.
    if context.ledger.criteria.is_empty() {
        return Ok(true);
    }
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET plan_criterion_revision = NULL, plan_checker_digest = NULL,
             plan_ledger_revision = NULL
         WHERE run_id = ?",
    )
    .bind(context.run.id.as_str())
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "UPDATE stateful_acceptance_ledgers
         SET workspace_generation = workspace_generation + 1
         WHERE run_id = ?",
    )
    .bind(context.run.id.as_str())
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

#[cfg(test)]
#[path = "acceptance_plan_tests.rs"]
mod tests;
