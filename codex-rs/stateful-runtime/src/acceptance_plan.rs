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

/// Interpreters whose first operand is the script they execute.
const SCRIPT_INTERPRETERS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "ksh", "python", "python3", "node", "ruby", "perl",
];

/// The host admits a criterion's check plan only from the ledger's own terms: an active
/// criterion with a check command, pinned artifacts and checker files, where the command's
/// argv executes one checker file (`<interpreter> <checker> [args]` or a direct path to the
/// checker script, see `executed_checker`) and the checker files were readable on the local
/// executor. The admission freezes the criterion revision, which fixes the exact command and
/// directory, and the checker bytes; receipts count only for that exact execution.
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
    if let Err(reason) =
        executed_checker(command, criterion.check_cwd.as_deref(), &criterion.checker)
    {
        return Err(refused(format!(
            "`{command}` does not execute one of {alias}'s checker files: {reason}"
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

/// Checks, lexically, that a command executes one of the checker files: the argv is plain
/// words with no shell syntax, and either an interpreter runs the checker as its first
/// operand or the first word is a path to the checker itself. The script path, resolved
/// against the check directory, must equal a declared checker path. A mention of a checker
/// that does not execute it (an echo, a comment, another file of the same name) is refused.
pub(crate) fn executed_checker(
    command: &str,
    check_cwd: Option<&str>,
    checker: &[String],
) -> Result<(), String> {
    if let Some(character) = command.chars().find(|character| {
        !(character.is_ascii_alphanumeric() || " -_./=:+,@%".contains(*character))
    }) {
        return Err(format!(
            "it contains `{character}`; an admitted check is plain words with no shell syntax, quoting or expansion"
        ));
    }
    let argv = command.split_whitespace().collect::<Vec<_>>();
    let Some(first) = argv.first() else {
        return Err("it is empty".to_string());
    };
    if first.contains('=') {
        return Err("it starts with an environment assignment".to_string());
    }
    // Only a bare interpreter name is an interpreter: a path to an executable that is merely
    // named like one (`./fake/sh`) is a direct program, so it must itself be the checker.
    let program = *first;
    let interpreter = !program.contains(['/', '\\'])
        && (SCRIPT_INTERPRETERS.contains(&program)
            || program.strip_prefix("python3.").is_some_and(|minor| {
                !minor.is_empty() && minor.chars().all(|character| character.is_ascii_digit())
            }));
    let script = if interpreter {
        match argv.get(1) {
            Some(script) if !script.starts_with('-') => *script,
            _ => {
                return Err(format!(
                    "`{program}` must run the checker file as its first operand, with no option before it"
                ));
            }
        }
    } else if first.contains('/') {
        first
    } else {
        return Err(
            "run the checker as `<interpreter> <checker>` (sh, bash, python, node, ...) or as `./<checker>`"
                .to_string(),
        );
    };
    if script.starts_with(['/', '\\']) {
        return Err("the checker must be named by a project-relative path".to_string());
    }
    let resolved =
        normalize(&format!("{}/{script}", check_cwd.unwrap_or("."))).ok_or_else(|| {
            "the script and the check directory must be project-relative paths inside the project"
                .to_string()
        })?;
    if checker
        .iter()
        .any(|path| normalize(path).as_deref() == Some(resolved.as_str()))
    {
        Ok(())
    } else {
        Err(format!(
            "it runs `{resolved}`, which is not one of the declared checker files"
        ))
    }
}

/// A project-relative path with `.` and `..` resolved lexically; `None` for an absolute path
/// or one that leaves the project.
fn normalize(path: &str) -> Option<String> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
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
    // The steering input is part of the acceptance request; it is incorporated only when
    // each of its sentences is covered by a user criterion (or an omission proposal, which
    // gates on its own) quoting it.
    let request = crate::acceptance_request::acceptance_request(connection, context.run).await?;
    let Some((_, range)) = request.parts.iter().find(|(id, _)| id == steering_id) else {
        return Err(AcceptanceError::Refused(format!(
            "steering {steering_id} is not part of this run's acceptance request"
        ))
        .into());
    };
    if let Some(sentence) =
        crate::acceptance_coverage::uncovered_sentences(&request.text, context.ledger)
            .into_iter()
            .find(|sentence| range.start <= sentence.start && sentence.end <= range.end)
    {
        let quote = sentence.quote(&request.text).unwrap_or_default();
        return Err(AcceptanceError::Refused(format!(
            "steering {steering_id} is not incorporated: \"{quote}\" is covered by no criterion; add a user criterion quoting it (requestQuote) with its check, then reconcile"
        ))
        .into());
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
