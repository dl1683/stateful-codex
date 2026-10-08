//! Criterion changes applied inside one ledger revision. User criteria (and accepted
//! omission proposals) come from the user's words: their requirement, statement and span are
//! fixed, they are always required, cannot be retired, and their artifacts and dependencies
//! can only grow. Only a host-validated receipt (a user steering instruction quoted exactly,
//! or a covering user criterion) can dismiss a user-bound proposal; model prose cannot.
//! Every change reports whether it changed durable state, so a no-op never counts as
//! progress.

use sqlx::SqliteConnection;

use crate::AcceptanceChange;
use crate::AcceptanceCriterion;
use crate::AcceptanceKind;
use crate::AcceptanceLedger;
use crate::AcceptanceOrigin;
use crate::AcceptanceState;
use crate::CriterionTerms;
use crate::DismissalReceipt;
use crate::EvidenceOutcome;
use crate::EvidenceSource;
use crate::RequestSpan;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStoreError;
use crate::TermsUpdate;
use crate::acceptance::AcceptanceError;
use crate::acceptance::MAX_ACCEPTANCE_CRITERIA;
use crate::acceptance::MAX_CRITERION_DEPENDENCIES;
use crate::acceptance::MAX_MILESTONE_BYTES;
use crate::acceptance::MAX_REASON_BYTES;
use crate::acceptance::MAX_STATEMENT_BYTES;
use crate::acceptance::validate_artifacts;
use crate::acceptance::validate_bounded;
use crate::acceptance::validate_check_command;
use crate::acceptance::validate_path;
use crate::acceptance_storage::EvidenceRow;
use crate::acceptance_storage::executes;
use crate::acceptance_storage::host_checked;
use crate::acceptance_storage::insert_evidence;
use crate::acceptance_storage::kind_name;
use crate::acceptance_storage::origin_name;
use crate::acceptance_storage::state_name;

/// The ledger revision a change is applied in, stamped on every criterion it touches.
pub(crate) struct ChangeContext<'a> {
    pub(crate) run: &'a StatefulRun,
    pub(crate) ledger: &'a AcceptanceLedger,
    pub(crate) ledger_revision: u64,
    pub(crate) source_id: &'a str,
    pub(crate) now: i64,
}

/// Applies one change; returns whether durable state changed.
pub(crate) async fn apply_change(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    change: AcceptanceChange,
) -> Result<bool, StatefulRunStoreError> {
    let ledger = context.ledger;
    let refused =
        |message: String| -> StatefulRunStoreError { AcceptanceError::Refused(message).into() };
    let existing = |ordinal: u32| {
        ledger
            .criterion(ordinal)
            .ok_or(AcceptanceError::UnknownCriterion(ordinal))
    };
    match change {
        AcceptanceChange::Add {
            origin,
            kind,
            statement,
            request_span,
            terms,
        } => {
            validate_bounded(&statement, MAX_STATEMENT_BYTES)?;
            let ordinal = next_ordinal(ledger);
            validate_terms(ledger, ordinal, &terms)?;
            let requirement = match (origin, request_span) {
                (AcceptanceOrigin::Omission, _) => {
                    return Err(refused(
                        "omission criteria are proposed only by the host omission check"
                            .to_string(),
                    ));
                }
                (AcceptanceOrigin::User, None) => {
                    return Err(refused(
                        "a user criterion must quote the exact goal text it comes from (requestQuote)"
                            .to_string(),
                    ));
                }
                (AcceptanceOrigin::User, Some(_)) if !terms.required => {
                    return Err(refused(
                        "a user criterion is always required; only the user can change the scope"
                            .to_string(),
                    ));
                }
                (_, Some(span)) => span
                    .quote(&context.run.value.goal)
                    .ok_or(AcceptanceError::InvalidSpan)?
                    .to_string(),
                (AcceptanceOrigin::Derived, None) => statement.clone(),
            };
            if ledger.criteria.len() >= MAX_ACCEPTANCE_CRITERIA {
                return Err(AcceptanceError::TooManyCriteria.into());
            }
            insert_criterion(
                connection,
                context,
                ordinal,
                NewCriterion {
                    origin,
                    kind,
                    state: AcceptanceState::Active,
                    statement: &statement,
                    requirement: &requirement,
                    span: request_span,
                    terms: &terms,
                },
            )
            .await?;
            Ok(true)
        }
        AcceptanceChange::Refine {
            ordinal,
            statement,
            required,
            terms,
        } => {
            let criterion = existing(ordinal)?;
            if criterion.state != AcceptanceState::Active {
                return Err(refused(format!("C{ordinal} is not active")));
            }
            if criterion.is_user_bound() {
                if statement.is_some() {
                    return Err(refused(format!(
                        "C{ordinal} comes from the user's words; its statement is fixed. Add a derived criterion for a refinement"
                    )));
                }
                if required == Some(false) {
                    return Err(refused(format!(
                        "C{ordinal} comes from the user's words and stays required; only the user can change the scope"
                    )));
                }
                if let Some(message) =
                    weakening_refusal(connection, &context.run.id, criterion, &terms).await?
                {
                    return Err(refused(message));
                }
            }
            update_criterion(
                connection,
                context,
                criterion,
                CriterionUpdate {
                    state: AcceptanceState::Active,
                    kind: criterion.kind,
                    statement,
                    required: required.unwrap_or(criterion.required),
                    terms,
                },
            )
            .await
        }
        AcceptanceChange::Accept {
            ordinal,
            kind,
            terms,
        } => {
            let criterion = existing(ordinal)?;
            if criterion.state != AcceptanceState::Proposed {
                return Err(refused(format!("C{ordinal} is not an omission proposal")));
            }
            update_criterion(
                connection,
                context,
                criterion,
                CriterionUpdate {
                    state: AcceptanceState::Active,
                    kind: kind.unwrap_or(criterion.kind),
                    statement: None,
                    required: true,
                    terms,
                },
            )
            .await
        }
        AcceptanceChange::Dismiss {
            ordinal,
            reason,
            receipt,
        } => {
            validate_bounded(&reason, MAX_REASON_BYTES)?;
            let criterion = existing(ordinal)?;
            if criterion.state != AcceptanceState::Proposed {
                return Err(refused(format!(
                    "C{ordinal} is not an omission proposal; only proposals can be dismissed"
                )));
            }
            validate_receipt(connection, context, criterion, &receipt).await?;
            dismiss_criterion(connection, context, criterion, &reason, &receipt).await?;
            Ok(true)
        }
        AcceptanceChange::Retire { ordinal, reason } => {
            validate_bounded(&reason, MAX_REASON_BYTES)?;
            let criterion = existing(ordinal)?;
            if criterion.is_user_bound() {
                return Err(refused(format!(
                    "C{ordinal} comes from the user's words and cannot be retired; if it cannot be met or verified, set the run blocked with a partial result that names it"
                )));
            }
            if criterion.state != AcceptanceState::Active {
                return Err(refused(format!("C{ordinal} is not active")));
            }
            if let Some(dependent) = ledger.criteria.iter().find(|other| {
                other.state == AcceptanceState::Active && other.depends_on.contains(&ordinal)
            }) {
                return Err(refused(format!(
                    "C{ordinal} cannot be retired while C{} depends on it",
                    dependent.ordinal
                )));
            }
            retire_criterion(connection, context, criterion, &reason).await?;
            Ok(true)
        }
        AcceptanceChange::Approve {
            ordinal,
            steering_id,
        } => {
            let criterion = existing(ordinal)?;
            let Some(command) = criterion.check_command.as_deref() else {
                return Err(refused(format!(
                    "C{ordinal} has no checkCommand to approve"
                )));
            };
            if criterion.state != AcceptanceState::Active {
                return Err(refused(format!("C{ordinal} is not active")));
            }
            let input = user_steering_text(connection, &context.run.id, &steering_id).await?;
            if !input.contains(command) {
                return Err(refused(format!(
                    "steering {steering_id} does not quote C{ordinal}'s exact check command; only the user's own instruction naming the command approves it"
                )));
            }
            if criterion.approved_by_steering.as_deref() == Some(steering_id.as_str()) {
                return Ok(false);
            }
            sqlx::query(
                "UPDATE stateful_acceptance_criteria
                 SET approved_by_steering = ?, ledger_revision = ?, updated_at_ms = ?
                 WHERE run_id = ? AND ordinal = ?",
            )
            .bind(&steering_id)
            .bind(count(context.ledger_revision)?)
            .bind(context.now)
            .bind(context.run.id.as_str())
            .bind(i64::from(ordinal))
            .execute(&mut *connection)
            .await?;
            Ok(true)
        }
        AcceptanceChange::Observe {
            ordinal,
            observation,
            artifact_digest,
        } => {
            let criterion = existing(ordinal)?;
            if criterion.kind != AcceptanceKind::Manual {
                return Err(refused(format!(
                    "C{ordinal} is not a manual criterion; a manual observation settles only a derived manual criterion pinned to its artifacts"
                )));
            }
            record_labelled_evidence(
                connection,
                context,
                criterion,
                EvidenceSource::Manual,
                &observation,
                artifact_digest.as_deref(),
            )
            .await?;
            Ok(true)
        }
        AcceptanceChange::NoCheck { ordinal, reason } => {
            record_labelled_evidence(
                connection,
                context,
                existing(ordinal)?,
                EvidenceSource::NoCheck,
                &reason,
                None,
            )
            .await?;
            Ok(true)
        }
    }
}

/// The text of a user steering instruction on this run that has not been rejected.
pub(crate) async fn user_steering_text(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    steering_id: &str,
) -> Result<String, StatefulRunStoreError> {
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT run_id, input, status FROM stateful_steering WHERE id = ?",
    )
    .bind(steering_id)
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some((run, input, status)) if run == run_id.as_str() && status != "rejected" => Ok(input),
        _ => Err(AcceptanceError::Refused(format!(
            "steering {steering_id} is not an unrejected user instruction on this run"
        ))
        .into()),
    }
}

async fn validate_receipt(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    proposal: &AcceptanceCriterion,
    receipt: &DismissalReceipt,
) -> Result<(), StatefulRunStoreError> {
    let refused =
        |message: String| -> StatefulRunStoreError { AcceptanceError::Refused(message).into() };
    match receipt {
        DismissalReceipt::UserSteering { steering_id, quote } => {
            validate_bounded(quote, MAX_REASON_BYTES)?;
            let input = user_steering_text(connection, &context.run.id, steering_id).await?;
            if !input.contains(quote.as_str()) {
                return Err(refused(format!(
                    "the quote is not exact text of steering {steering_id}"
                )));
            }
            Ok(())
        }
        DismissalReceipt::CoveredBy(covering) => {
            let span = proposal.request_span.ok_or(AcceptanceError::InvalidSpan)?;
            let covers = context.ledger.criterion(*covering).is_some_and(|other| {
                other.ordinal != proposal.ordinal
                    && other.is_user_bound()
                    && other.state == AcceptanceState::Active
                    && other.required
                    && other
                        .request_span
                        .is_some_and(|other| other.start <= span.start && span.end <= other.end)
            });
            if covers {
                Ok(())
            } else {
                Err(refused(format!(
                    "C{covering} is not an active required user criterion whose quote covers C{}'s text",
                    proposal.ordinal
                )))
            }
        }
    }
}

/// Inserts one host omission proposal: a required user-bound criterion awaiting review.
pub(crate) async fn insert_omission_proposal(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    ordinal: u32,
    statement: &str,
    span: RequestSpan,
) -> Result<(), StatefulRunStoreError> {
    validate_bounded(statement, MAX_STATEMENT_BYTES)?;
    let requirement = span
        .quote(&context.run.value.goal)
        .ok_or(AcceptanceError::InvalidSpan)?
        .to_string();
    insert_criterion(
        connection,
        context,
        ordinal,
        NewCriterion {
            origin: AcceptanceOrigin::Omission,
            kind: AcceptanceKind::Constraint,
            state: AcceptanceState::Proposed,
            statement,
            requirement: &requirement,
            span: Some(span),
            terms: &CriterionTerms {
                required: true,
                ..CriterionTerms::default()
            },
        },
    )
    .await
}

/// Why a terms update would weaken a user-bound criterion, if it would.
async fn weakening_refusal(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    criterion: &AcceptanceCriterion,
    terms: &TermsUpdate,
) -> Result<Option<String>, StatefulRunStoreError> {
    let alias = criterion.alias();
    let dropped = |current: &[String], next: Option<&Vec<String>>| {
        next.and_then(|next| current.iter().find(|item| !next.contains(item)).cloned())
    };
    if let Some(artifact) = dropped(&criterion.artifacts, terms.artifacts.as_ref()) {
        return Ok(Some(format!(
            "{alias} comes from the user's words; artifacts can be added but {artifact} cannot be removed"
        )));
    }
    if let Some(next) = &terms.depends_on
        && let Some(dependency) = criterion
            .depends_on
            .iter()
            .find(|dependency| !next.contains(dependency))
    {
        return Ok(Some(format!(
            "{alias} comes from the user's words; its dependency on C{dependency} cannot be removed"
        )));
    }
    if terms.milestone.is_some() && criterion.milestone.is_some() {
        return Ok(Some(format!(
            "{alias} comes from the user's words; its milestone link cannot be replaced"
        )));
    }
    let check_change = terms.check_command.is_some()
        || terms.expected_observation.is_some()
        || terms.check_cwd.is_some();
    if check_change
        && criterion.check_command.is_some()
        && host_checked(connection, run_id, criterion.ordinal).await?
    {
        return Ok(Some(format!(
            "{alias} comes from the user's words and its check already ran; the check, its directory and its expected observation cannot be replaced. Repair the work, or report the failure"
        )));
    }
    Ok(None)
}

fn validate_terms(
    ledger: &AcceptanceLedger,
    ordinal: u32,
    terms: &CriterionTerms,
) -> Result<(), StatefulRunStoreError> {
    validate_artifacts(&terms.artifacts)?;
    if let Some(command) = terms.check_command.as_deref() {
        validate_check_command(command)?;
        if terms.expected_observation.is_none() || terms.artifacts.is_empty() {
            return Err(AcceptanceError::CheckWithoutExpectation.into());
        }
    }
    if let Some(expected) = terms.expected_observation.as_deref() {
        validate_bounded(expected, MAX_REASON_BYTES)?;
    }
    if let Some(cwd) = terms.check_cwd.as_deref() {
        validate_path(cwd)?;
    }
    if let Some(milestone) = terms.milestone.as_deref() {
        validate_bounded(milestone, MAX_MILESTONE_BYTES)?;
    }
    let mut seen = std::collections::BTreeSet::new();
    if terms.depends_on.len() > MAX_CRITERION_DEPENDENCIES
        || terms.depends_on.iter().any(|dependency| {
            !seen.insert(*dependency)
                || *dependency >= ordinal
                || ledger.criterion(*dependency).is_none()
        })
    {
        return Err(AcceptanceError::InvalidDependency.into());
    }
    Ok(())
}

/// A manual observation or a no-safe-check statement. Both are agent-written and labelled as
/// such; neither is accepted for a criterion that declares a host check, and neither is
/// recorded before the run executes (a pending Socratic run does not check anything).
async fn record_labelled_evidence(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    criterion: &AcceptanceCriterion,
    source: EvidenceSource,
    detail: &str,
    artifact_digest: Option<&str>,
) -> Result<(), StatefulRunStoreError> {
    validate_bounded(detail, MAX_REASON_BYTES)?;
    let alias = criterion.alias();
    let run = context.run;
    if !executes(run.status) {
        return Err(AcceptanceError::Refused(format!(
            "evidence for {alias} is recorded only while the run executes; the run is {:?}",
            run.status
        ))
        .into());
    }
    if criterion.state != AcceptanceState::Active {
        return Err(AcceptanceError::Refused(format!("{alias} is not active")).into());
    }
    if let Some(command) = criterion.check_command.as_deref() {
        return Err(AcceptanceError::Refused(format!(
            "{alias} declares the host check `{command}`; run it with the shell tool. If the approval policy declines it, the host records it as unavailable"
        ))
        .into());
    }
    let outcome = match source {
        EvidenceSource::Manual => EvidenceOutcome::Observed,
        EvidenceSource::NoCheck | EvidenceSource::HostCommand => EvidenceOutcome::Unavailable,
    };
    insert_evidence(
        connection,
        &run.id,
        criterion,
        EvidenceRow {
            source,
            outcome,
            command: None,
            exit_code: None,
            output_tail: None,
            output_digest: None,
            artifact_digest,
            detail: Some(detail),
            workspace_generation: context.ledger.workspace_generation,
            source_id: context.source_id,
        },
        context.now,
    )
    .await
}

struct NewCriterion<'a> {
    origin: AcceptanceOrigin,
    kind: AcceptanceKind,
    state: AcceptanceState,
    statement: &'a str,
    requirement: &'a str,
    span: Option<RequestSpan>,
    terms: &'a CriterionTerms,
}

fn count(value: impl TryInto<i64>) -> Result<i64, StatefulRunStoreError> {
    value
        .try_into()
        .map_err(|_| StatefulRunStoreError::CountOverflow)
}

async fn insert_criterion(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    ordinal: u32,
    row: NewCriterion<'_>,
) -> Result<(), StatefulRunStoreError> {
    let (start, end) = match row.span {
        Some(span) => (Some(count(span.start)?), Some(count(span.end)?)),
        None => (None, None),
    };
    let run_id = &context.run.id;
    sqlx::query(
        "INSERT INTO stateful_acceptance_criteria (
            run_id, ordinal, criterion_id, origin, kind, state, statement, requirement,
            required, depends_on_json, milestone, expected_observation, span_start, span_end,
            artifacts_json, check_command, check_cwd, approved_by_steering,
            dismissal_steering_id, dismissal_quote, dismissal_covered_by, note, revision,
            ledger_revision, created_at_ms, updated_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL,
                   NULL, 1, ?, ?, ?)",
    )
    .bind(run_id.as_str())
    .bind(i64::from(ordinal))
    .bind(format!("{run_id}#C{ordinal}"))
    .bind(origin_name(row.origin))
    .bind(kind_name(row.kind))
    .bind(state_name(row.state))
    .bind(row.statement)
    .bind(row.requirement)
    .bind(row.terms.required)
    .bind(serde_json::to_string(&row.terms.depends_on)?)
    .bind(row.terms.milestone.as_deref())
    .bind(row.terms.expected_observation.as_deref())
    .bind(start)
    .bind(end)
    .bind(serde_json::to_string(&row.terms.artifacts)?)
    .bind(row.terms.check_command.as_deref())
    .bind(row.terms.check_cwd.as_deref())
    .bind(count(context.ledger_revision)?)
    .bind(context.now)
    .bind(context.now)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

struct CriterionUpdate {
    state: AcceptanceState,
    kind: AcceptanceKind,
    statement: Option<String>,
    required: bool,
    terms: TermsUpdate,
}

/// Applies a refinement or acceptance; any change of what the criterion checks bumps its
/// revision (earlier evidence becomes non-current) and clears the user's approval of the
/// previous check. Returns whether anything changed.
async fn update_criterion(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    criterion: &AcceptanceCriterion,
    update: CriterionUpdate,
) -> Result<bool, StatefulRunStoreError> {
    if let Some(statement) = &update.statement {
        validate_bounded(statement, MAX_STATEMENT_BYTES)?;
    }
    let terms = CriterionTerms {
        required: update.required,
        depends_on: update
            .terms
            .depends_on
            .unwrap_or_else(|| criterion.depends_on.clone()),
        milestone: update
            .terms
            .milestone
            .or_else(|| criterion.milestone.clone()),
        artifacts: update
            .terms
            .artifacts
            .unwrap_or_else(|| criterion.artifacts.clone()),
        check_command: update
            .terms
            .check_command
            .or_else(|| criterion.check_command.clone()),
        check_cwd: update
            .terms
            .check_cwd
            .or_else(|| criterion.check_cwd.clone()),
        expected_observation: update
            .terms
            .expected_observation
            .or_else(|| criterion.expected_observation.clone()),
    };
    validate_terms(context.ledger, criterion.ordinal, &terms)?;
    let statement = update
        .statement
        .unwrap_or_else(|| criterion.statement.clone());
    // Derived criteria state their own requirement; a user-bound one keeps the quoted text.
    let requirement = if criterion.is_user_bound() {
        criterion.requirement.clone()
    } else {
        statement.clone()
    };
    let method_unchanged = terms.artifacts == criterion.artifacts
        && terms.check_command == criterion.check_command
        && terms.check_cwd == criterion.check_cwd
        && terms.expected_observation == criterion.expected_observation;
    let unchanged = method_unchanged
        && statement == criterion.statement
        && terms.required == criterion.required
        && terms.depends_on == criterion.depends_on
        && terms.milestone == criterion.milestone
        && update.kind == criterion.kind
        && update.state == criterion.state;
    if unchanged {
        return Ok(false);
    }
    let approval = if method_unchanged {
        criterion.approved_by_steering.clone()
    } else {
        None
    };
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET state = ?, kind = ?, statement = ?, requirement = ?, required = ?,
             depends_on_json = ?, milestone = ?, expected_observation = ?, artifacts_json = ?,
             check_command = ?, check_cwd = ?, approved_by_steering = ?,
             revision = revision + 1, ledger_revision = ?, updated_at_ms = ?
         WHERE run_id = ? AND ordinal = ?",
    )
    .bind(state_name(update.state))
    .bind(kind_name(update.kind))
    .bind(statement)
    .bind(requirement)
    .bind(terms.required)
    .bind(serde_json::to_string(&terms.depends_on)?)
    .bind(terms.milestone)
    .bind(terms.expected_observation)
    .bind(serde_json::to_string(&terms.artifacts)?)
    .bind(terms.check_command)
    .bind(terms.check_cwd)
    .bind(approval)
    .bind(count(context.ledger_revision)?)
    .bind(context.now)
    .bind(context.run.id.as_str())
    .bind(i64::from(criterion.ordinal))
    .execute(&mut *connection)
    .await?;
    Ok(true)
}

async fn dismiss_criterion(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    criterion: &AcceptanceCriterion,
    reason: &str,
    receipt: &DismissalReceipt,
) -> Result<(), StatefulRunStoreError> {
    let (steering_id, quote, covered_by) = match receipt {
        DismissalReceipt::UserSteering { steering_id, quote } => {
            (Some(steering_id.as_str()), Some(quote.as_str()), None)
        }
        DismissalReceipt::CoveredBy(ordinal) => (None, None, Some(i64::from(*ordinal))),
    };
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET state = 'dismissed', note = ?, dismissal_steering_id = ?, dismissal_quote = ?,
             dismissal_covered_by = ?, revision = revision + 1, ledger_revision = ?,
             updated_at_ms = ?
         WHERE run_id = ? AND ordinal = ?",
    )
    .bind(reason)
    .bind(steering_id)
    .bind(quote)
    .bind(covered_by)
    .bind(count(context.ledger_revision)?)
    .bind(context.now)
    .bind(context.run.id.as_str())
    .bind(i64::from(criterion.ordinal))
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn retire_criterion(
    connection: &mut SqliteConnection,
    context: &ChangeContext<'_>,
    criterion: &AcceptanceCriterion,
    reason: &str,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET state = 'retired', note = ?, revision = revision + 1, ledger_revision = ?,
             updated_at_ms = ?
         WHERE run_id = ? AND ordinal = ?",
    )
    .bind(reason)
    .bind(count(context.ledger_revision)?)
    .bind(context.now)
    .bind(context.run.id.as_str())
    .bind(i64::from(criterion.ordinal))
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) fn next_ordinal(ledger: &AcceptanceLedger) -> u32 {
    ledger
        .criteria
        .iter()
        .map(|criterion| criterion.ordinal)
        .max()
        .unwrap_or_default()
        + 1
}
