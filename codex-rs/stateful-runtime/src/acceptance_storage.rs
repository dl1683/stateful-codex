//! Durable acceptance ledgers: criteria revisions, bounded evidence and the completion gate
//! evaluated inside the terminal transaction.

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::AcceptanceChange;
use crate::AcceptanceCommit;
use crate::AcceptanceCriterion;
use crate::AcceptanceEvidence;
use crate::AcceptanceKind;
use crate::AcceptanceLedger;
use crate::AcceptanceOrigin;
use crate::AcceptanceState;
use crate::CommandEvidence;
use crate::EvidenceOutcome;
use crate::EvidenceSource;
use crate::RequestSpan;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::acceptance::AcceptanceError;
use crate::acceptance::MAX_ACCEPTANCE_CHANGES;
use crate::acceptance::MAX_ACCEPTANCE_CRITERIA;
use crate::acceptance::MAX_OUTPUT_TAIL_BYTES;
use crate::acceptance::MAX_REASON_BYTES;
use crate::acceptance::MAX_STATEMENT_BYTES;
use crate::acceptance::unmet_criteria;
use crate::acceptance::validate_artifacts;
use crate::acceptance::validate_bounded;
use crate::acceptance::validate_check_command;
use crate::storage::load_run;
use crate::storage::unix_timestamp_millis;

/// Evidence rows kept per criterion; only the newest one gates, the rest are history.
const RETAINED_EVIDENCE_PER_CRITERION: i64 = 4;

impl StatefulRunStore {
    /// The run's ledger; an empty ledger at revision 0 when none was recorded.
    pub async fn acceptance_ledger(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<AcceptanceLedger, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_ledger(&mut connection, run_id).await
    }

    /// Applies criterion changes and labelled evidence as one ledger revision.
    pub async fn revise_acceptance(
        &self,
        run_id: &StatefulRunId,
        expected_revision: u64,
        changes: Vec<AcceptanceChange>,
        source_id: &str,
    ) -> Result<AcceptanceLedger, StatefulRunStoreError> {
        if changes.is_empty() || changes.len() > MAX_ACCEPTANCE_CHANGES {
            return Err(AcceptanceError::InvalidChangeCount.into());
        }
        crate::run::validate_source_id(source_id)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        ensure_ledger(&mut transaction, run_id).await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        if ledger.revision != expected_revision {
            return Err(StatefulRunStoreError::AcceptanceRevisionConflict {
                expected: expected_revision,
                actual: ledger.revision,
            });
        }
        let now = unix_timestamp_millis()?;
        for change in changes {
            let ledger = load_ledger(&mut transaction, run_id).await?;
            apply_change(&mut transaction, &run, &ledger, change, source_id, now).await?;
        }
        bump_revision(&mut transaction, run_id, now).await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger)
    }

    /// Runs the host omission check once per run: records its proposals for review and marks
    /// the check done. A ledger already checked is returned unchanged.
    pub async fn record_omission_check(
        &self,
        run_id: &StatefulRunId,
        proposals: Vec<(String, RequestSpan)>,
    ) -> Result<AcceptanceLedger, StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        ensure_ledger(&mut transaction, run_id).await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        if ledger.omission_checked {
            transaction.commit().await?;
            return Ok(ledger);
        }
        let now = unix_timestamp_millis()?;
        let slots = MAX_ACCEPTANCE_CRITERIA.saturating_sub(ledger.criteria.len());
        let mut next = next_ordinal(&ledger);
        let mut added = 0;
        for (statement, span) in proposals.into_iter().take(slots) {
            validate_bounded(&statement, MAX_STATEMENT_BYTES)?;
            if span.quote(&run.value.goal).is_none() {
                return Err(AcceptanceError::InvalidSpan.into());
            }
            insert_criterion(
                &mut transaction,
                run_id,
                next,
                NewRow {
                    origin: AcceptanceOrigin::Omission,
                    kind: AcceptanceKind::Constraint,
                    state: AcceptanceState::Proposed,
                    statement: &statement,
                    span: Some(span),
                    artifacts: &[],
                    check_command: None,
                },
                now,
            )
            .await?;
            next += 1;
            added += 1;
        }
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET omission_checked = 1, revision = revision + ?, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(i64::from(added > 0))
        .bind(now)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger)
    }

    /// Records observed check executions. Evidence binds only to the active criterion revision
    /// it was matched against and only while the run executes; anything else is skipped.
    /// Returns the number recorded.
    pub async fn record_command_evidence(
        &self,
        run_id: &StatefulRunId,
        evidence: Vec<CommandEvidence>,
    ) -> Result<usize, StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let Some(run) = load_run(&mut transaction, run_id).await? else {
            return Ok(0);
        };
        if !executes(run.status) {
            return Ok(0);
        }
        let ledger = load_ledger(&mut transaction, run_id).await?;
        let now = unix_timestamp_millis()?;
        let mut recorded = 0;
        for observed in evidence {
            let Some(criterion) = ledger.criterion(observed.ordinal) else {
                continue;
            };
            if criterion.state != AcceptanceState::Active
                || criterion.revision != observed.criterion_revision
                || criterion.check_command.as_deref() != Some(observed.command.as_str())
            {
                continue;
            }
            insert_evidence(
                &mut transaction,
                run_id,
                criterion,
                EvidenceRow {
                    source: EvidenceSource::HostCommand,
                    outcome: observed.outcome,
                    command: Some(&observed.command),
                    exit_code: observed.exit_code,
                    output_tail: Some(&bounded_tail(&observed.output_tail)),
                    output_digest: Some(&observed.output_digest),
                    artifact_digest: observed.artifact_digest.as_deref(),
                    detail: observed.detail.as_deref(),
                    workspace_generation: ledger.workspace_generation,
                    source_id: &observed.source_id,
                },
                now,
            )
            .await?;
            recorded += 1;
        }
        if recorded > 0 {
            bump_revision(&mut transaction, run_id, now).await?;
        }
        transaction.commit().await?;
        Ok(recorded)
    }

    /// Records one host-observed workspace mutation that was not a declared check, which
    /// makes every earlier passing check and manual observation stale.
    pub async fn bump_workspace_generation(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<(), StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let Some(run) = load_run(&mut transaction, run_id).await? else {
            return Ok(());
        };
        if run.status.is_terminal() {
            return Ok(());
        }
        ensure_ledger(&mut transaction, run_id).await?;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET workspace_generation = workspace_generation + 1, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(unix_timestamp_millis()?)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Counts a rejected completion. The count is consecutive rejections at the same ledger
    /// revision: any criterion change or new evidence starts it again at one.
    pub async fn note_rejected_completion(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<u32, StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if load_run(&mut transaction, run_id).await?.is_none() {
            return Err(StatefulRunStoreError::RunNotFound(run_id.to_string()));
        }
        ensure_ledger(&mut transaction, run_id).await?;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET stalled_completions = CASE WHEN stalled_at_revision = revision
                     THEN stalled_completions + 1 ELSE 1 END,
                 stalled_at_revision = revision, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(unix_timestamp_millis()?)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger.stalled_completions)
    }
}

/// Evaluates the gate inside the terminal transaction. With a commit, the ledger revision and
/// workspace generation must be exactly the ones the caller validated against; without one
/// (direct store callers), declared artifacts count as unread.
pub(crate) async fn enforce_acceptance_gate(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    commit: Option<&AcceptanceCommit>,
) -> Result<(), StatefulRunStoreError> {
    let ledger = load_ledger(connection, run_id).await?;
    let default_commit = AcceptanceCommit::default();
    let commit = match commit {
        Some(commit) => {
            if commit.ledger_revision != ledger.revision
                || commit.workspace_generation != ledger.workspace_generation
            {
                return Err(StatefulRunStoreError::AcceptanceChanged);
            }
            if commit.omission_required && !ledger.omission_checked {
                return Err(StatefulRunStoreError::AcceptanceGate(
                    "the omission check has not run for this substantial task".to_string(),
                ));
            }
            commit
        }
        None => &default_commit,
    };
    let unmet = unmet_criteria(&ledger, commit);
    if unmet.is_empty() {
        return Ok(());
    }
    let summary = unmet
        .iter()
        .map(|(ordinal, reason)| format!("C{ordinal}: {reason}"))
        .collect::<Vec<_>>()
        .join("; ");
    Err(StatefulRunStoreError::AcceptanceGate(summary))
}

fn executes(status: StatefulRunStatus) -> bool {
    matches!(
        status,
        StatefulRunStatus::Running | StatefulRunStatus::Blocked
    )
}

async fn apply_change(
    connection: &mut SqliteConnection,
    run: &StatefulRun,
    ledger: &AcceptanceLedger,
    change: AcceptanceChange,
    source_id: &str,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
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
            artifacts,
            check_command,
        } => {
            validate_bounded(&statement, MAX_STATEMENT_BYTES)?;
            validate_artifacts(&artifacts)?;
            if let Some(command) = check_command.as_deref() {
                validate_check_command(command)?;
            }
            match (origin, request_span) {
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
                (_, Some(span)) if span.quote(&run.value.goal).is_none() => {
                    return Err(AcceptanceError::InvalidSpan.into());
                }
                _ => {}
            }
            if ledger.criteria.len() >= MAX_ACCEPTANCE_CRITERIA {
                return Err(AcceptanceError::TooManyCriteria.into());
            }
            insert_criterion(
                connection,
                &run.id,
                next_ordinal(ledger),
                NewRow {
                    origin,
                    kind,
                    state: AcceptanceState::Active,
                    statement: &statement,
                    span: request_span,
                    artifacts: &artifacts,
                    check_command: check_command.as_deref(),
                },
                now,
            )
            .await
        }
        AcceptanceChange::Refine {
            ordinal,
            statement,
            artifacts,
            check_command,
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
                if let Some(artifacts) = &artifacts
                    && let Some(dropped) = criterion
                        .artifacts
                        .iter()
                        .find(|artifact| !artifacts.contains(artifact))
                {
                    return Err(refused(format!(
                        "C{ordinal} comes from the user's words; artifacts can be added but {dropped} cannot be removed"
                    )));
                }
                if check_command.is_some()
                    && criterion.check_command.is_some()
                    && host_checked(connection, &run.id, ordinal).await?
                {
                    return Err(refused(format!(
                        "C{ordinal} comes from the user's words and its check already ran; the check cannot be replaced. Repair the work, or report the failure"
                    )));
                }
            }
            update_criterion(
                connection,
                &run.id,
                criterion,
                CriterionUpdate {
                    state: AcceptanceState::Active,
                    kind: criterion.kind,
                    statement,
                    artifacts,
                    check_command,
                    note: None,
                },
                now,
            )
            .await
        }
        AcceptanceChange::Accept {
            ordinal,
            kind,
            artifacts,
            check_command,
        } => {
            let criterion = existing(ordinal)?;
            if criterion.state != AcceptanceState::Proposed {
                return Err(refused(format!("C{ordinal} is not an omission proposal")));
            }
            update_criterion(
                connection,
                &run.id,
                criterion,
                CriterionUpdate {
                    state: AcceptanceState::Active,
                    kind: kind.unwrap_or(criterion.kind),
                    statement: None,
                    artifacts,
                    check_command,
                    note: None,
                },
                now,
            )
            .await
        }
        AcceptanceChange::Dismiss { ordinal, reason } => {
            validate_bounded(&reason, MAX_REASON_BYTES)?;
            let criterion = existing(ordinal)?;
            if criterion.state != AcceptanceState::Proposed {
                return Err(refused(format!(
                    "C{ordinal} is not an omission proposal; only proposals can be dismissed"
                )));
            }
            close_criterion(
                connection,
                &run.id,
                criterion,
                AcceptanceState::Dismissed,
                &reason,
                now,
            )
            .await
        }
        AcceptanceChange::Retire { ordinal, reason } => {
            validate_bounded(&reason, MAX_REASON_BYTES)?;
            let criterion = existing(ordinal)?;
            if criterion.is_user_bound() {
                return Err(refused(format!(
                    "C{ordinal} comes from the user's words and cannot be retired; if it cannot be met or checked, record noCheck with the reason so completion discloses it"
                )));
            }
            if criterion.state != AcceptanceState::Active {
                return Err(refused(format!("C{ordinal} is not active")));
            }
            close_criterion(
                connection,
                &run.id,
                criterion,
                AcceptanceState::Retired,
                &reason,
                now,
            )
            .await
        }
        AcceptanceChange::Observe {
            ordinal,
            observation,
        } => {
            record_labelled_evidence(
                connection,
                run,
                ledger,
                existing(ordinal)?,
                EvidenceSource::Manual,
                &observation,
                source_id,
                now,
            )
            .await
        }
        AcceptanceChange::NoCheck { ordinal, reason } => {
            record_labelled_evidence(
                connection,
                run,
                ledger,
                existing(ordinal)?,
                EvidenceSource::NoCheck,
                &reason,
                source_id,
                now,
            )
            .await
        }
    }
}

/// A manual observation or a no-safe-check statement. Both are agent-written and labelled as
/// such; neither is accepted for a criterion that declares a host check, and neither is
/// recorded before the run executes (a pending Socratic run does not check anything).
#[expect(
    clippy::too_many_arguments,
    reason = "one private call shape for both labels"
)]
async fn record_labelled_evidence(
    connection: &mut SqliteConnection,
    run: &StatefulRun,
    ledger: &AcceptanceLedger,
    criterion: &AcceptanceCriterion,
    source: EvidenceSource,
    detail: &str,
    source_id: &str,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    validate_bounded(detail, MAX_REASON_BYTES)?;
    let alias = criterion.alias();
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
            artifact_digest: None,
            detail: Some(detail),
            workspace_generation: ledger.workspace_generation,
            source_id,
        },
        now,
    )
    .await
}

struct NewRow<'a> {
    origin: AcceptanceOrigin,
    kind: AcceptanceKind,
    state: AcceptanceState,
    statement: &'a str,
    span: Option<RequestSpan>,
    artifacts: &'a [String],
    check_command: Option<&'a str>,
}

async fn insert_criterion(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    ordinal: u32,
    row: NewRow<'_>,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    let (start, end) = match row.span {
        Some(span) => (
            Some(i64::try_from(span.start).map_err(|_| StatefulRunStoreError::CountOverflow)?),
            Some(i64::try_from(span.end).map_err(|_| StatefulRunStoreError::CountOverflow)?),
        ),
        None => (None, None),
    };
    sqlx::query(
        "INSERT INTO stateful_acceptance_criteria (
            run_id, ordinal, origin, kind, state, statement, span_start, span_end,
            artifacts_json, check_command, note, revision, created_at_ms, updated_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, 1, ?, ?)",
    )
    .bind(run_id.as_str())
    .bind(i64::from(ordinal))
    .bind(origin_name(row.origin))
    .bind(kind_name(row.kind))
    .bind(state_name(row.state))
    .bind(row.statement)
    .bind(start)
    .bind(end)
    .bind(serde_json::to_string(row.artifacts)?)
    .bind(row.check_command)
    .bind(now)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

struct CriterionUpdate {
    state: AcceptanceState,
    kind: AcceptanceKind,
    statement: Option<String>,
    artifacts: Option<Vec<String>>,
    check_command: Option<String>,
    note: Option<String>,
}

async fn update_criterion(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    criterion: &AcceptanceCriterion,
    update: CriterionUpdate,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    if let Some(statement) = &update.statement {
        validate_bounded(statement, MAX_STATEMENT_BYTES)?;
    }
    if let Some(artifacts) = &update.artifacts {
        validate_artifacts(artifacts)?;
    }
    if let Some(command) = &update.check_command {
        validate_check_command(command)?;
    }
    let statement = update
        .statement
        .unwrap_or_else(|| criterion.statement.clone());
    let artifacts = update
        .artifacts
        .unwrap_or_else(|| criterion.artifacts.clone());
    let check_command = update
        .check_command
        .or_else(|| criterion.check_command.clone());
    let changed = statement != criterion.statement
        || artifacts != criterion.artifacts
        || check_command != criterion.check_command
        || update.kind != criterion.kind
        || update.state != criterion.state;
    if !changed {
        return Ok(());
    }
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET state = ?, kind = ?, statement = ?, artifacts_json = ?, check_command = ?,
             note = ?, revision = revision + 1, updated_at_ms = ?
         WHERE run_id = ? AND ordinal = ?",
    )
    .bind(state_name(update.state))
    .bind(kind_name(update.kind))
    .bind(statement)
    .bind(serde_json::to_string(&artifacts)?)
    .bind(check_command)
    .bind(update.note)
    .bind(now)
    .bind(run_id.as_str())
    .bind(i64::from(criterion.ordinal))
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn close_criterion(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    criterion: &AcceptanceCriterion,
    state: AcceptanceState,
    reason: &str,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "UPDATE stateful_acceptance_criteria
         SET state = ?, note = ?, revision = revision + 1, updated_at_ms = ?
         WHERE run_id = ? AND ordinal = ?",
    )
    .bind(state_name(state))
    .bind(reason)
    .bind(now)
    .bind(run_id.as_str())
    .bind(i64::from(criterion.ordinal))
    .execute(&mut *connection)
    .await?;
    Ok(())
}

struct EvidenceRow<'a> {
    source: EvidenceSource,
    outcome: EvidenceOutcome,
    command: Option<&'a str>,
    exit_code: Option<i32>,
    output_tail: Option<&'a str>,
    output_digest: Option<&'a str>,
    artifact_digest: Option<&'a str>,
    detail: Option<&'a str>,
    workspace_generation: u64,
    source_id: &'a str,
}

async fn insert_evidence(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    criterion: &AcceptanceCriterion,
    row: EvidenceRow<'_>,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "INSERT INTO stateful_acceptance_evidence (
            run_id, ordinal, source, outcome, command, exit_code, output_tail, output_digest,
            artifact_digest, detail, workspace_generation, criterion_revision, source_id,
            observed_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id.as_str())
    .bind(i64::from(criterion.ordinal))
    .bind(source_name(row.source))
    .bind(outcome_name(row.outcome))
    .bind(row.command)
    .bind(row.exit_code)
    .bind(row.output_tail)
    .bind(row.output_digest)
    .bind(row.artifact_digest)
    .bind(row.detail)
    .bind(
        i64::try_from(row.workspace_generation)
            .map_err(|_| StatefulRunStoreError::CountOverflow)?,
    )
    .bind(i64::try_from(criterion.revision).map_err(|_| StatefulRunStoreError::CountOverflow)?)
    .bind(row.source_id)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    sqlx::query(
        "DELETE FROM stateful_acceptance_evidence
         WHERE run_id = ? AND ordinal = ? AND sequence NOT IN (
             SELECT sequence FROM stateful_acceptance_evidence
             WHERE run_id = ? AND ordinal = ? ORDER BY sequence DESC LIMIT ?
         )",
    )
    .bind(run_id.as_str())
    .bind(i64::from(criterion.ordinal))
    .bind(run_id.as_str())
    .bind(i64::from(criterion.ordinal))
    .bind(RETAINED_EVIDENCE_PER_CRITERION)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn host_checked(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    ordinal: u32,
) -> Result<bool, StatefulRunStoreError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM stateful_acceptance_evidence
         WHERE run_id = ? AND ordinal = ? AND source = 'hostCommand' LIMIT 1",
    )
    .bind(run_id.as_str())
    .bind(i64::from(ordinal))
    .fetch_optional(&mut *connection)
    .await?
    .is_some())
}

async fn ensure_ledger(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "INSERT OR IGNORE INTO stateful_acceptance_ledgers (
            run_id, revision, workspace_generation, omission_checked, stalled_completions,
            stalled_at_revision, updated_at_ms
         ) VALUES (?, 0, 0, 0, 0, 0, ?)",
    )
    .bind(run_id.as_str())
    .bind(unix_timestamp_millis()?)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn bump_revision(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    now: i64,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "UPDATE stateful_acceptance_ledgers
         SET revision = revision + 1, updated_at_ms = ?
         WHERE run_id = ?",
    )
    .bind(now)
    .bind(run_id.as_str())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

fn next_ordinal(ledger: &AcceptanceLedger) -> u32 {
    ledger
        .criteria
        .iter()
        .map(|criterion| criterion.ordinal)
        .max()
        .unwrap_or_default()
        + 1
}

fn bounded_tail(output: &str) -> String {
    if output.len() <= MAX_OUTPUT_TAIL_BYTES {
        return output.to_string();
    }
    let mut start = output.len() - MAX_OUTPUT_TAIL_BYTES;
    while !output.is_char_boundary(start) {
        start += 1;
    }
    output[start..].to_string()
}

#[derive(FromRow)]
struct StoredLedger {
    revision: i64,
    workspace_generation: i64,
    omission_checked: i64,
    stalled_completions: i64,
}

#[derive(FromRow)]
struct StoredCriterion {
    ordinal: i64,
    origin: String,
    kind: String,
    state: String,
    statement: String,
    span_start: Option<i64>,
    span_end: Option<i64>,
    artifacts_json: String,
    check_command: Option<String>,
    note: Option<String>,
    revision: i64,
}

#[derive(FromRow)]
struct StoredEvidence {
    sequence: i64,
    ordinal: i64,
    source: String,
    outcome: String,
    command: Option<String>,
    exit_code: Option<i64>,
    output_tail: Option<String>,
    output_digest: Option<String>,
    artifact_digest: Option<String>,
    detail: Option<String>,
    workspace_generation: i64,
    criterion_revision: i64,
    source_id: String,
    observed_at_ms: i64,
}

pub(crate) async fn load_ledger(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<AcceptanceLedger, StatefulRunStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredLedger>(
        "SELECT revision, workspace_generation, omission_checked, stalled_completions
         FROM stateful_acceptance_ledgers WHERE run_id = ?",
    )
    .bind(run_id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(AcceptanceLedger::empty(run_id.clone()));
    };
    let criteria = sqlx::query_as::<_, StoredCriterion>(
        "SELECT ordinal, origin, kind, state, statement, span_start, span_end, artifacts_json,
                check_command, note, revision
         FROM stateful_acceptance_criteria WHERE run_id = ? ORDER BY ordinal LIMIT ?",
    )
    .bind(run_id.as_str())
    .bind(i64::try_from(MAX_ACCEPTANCE_CRITERIA).map_err(|_| StatefulRunStoreError::CountOverflow)?)
    .fetch_all(&mut *connection)
    .await?;
    let evidence = sqlx::query_as::<_, StoredEvidence>(
        "SELECT sequence, ordinal, source, outcome, command, exit_code, output_tail,
                output_digest, artifact_digest, detail, workspace_generation,
                criterion_revision, source_id, observed_at_ms
         FROM stateful_acceptance_evidence AS evidence
         WHERE run_id = ? AND sequence = (
             SELECT MAX(sequence) FROM stateful_acceptance_evidence AS newest
             WHERE newest.run_id = evidence.run_id AND newest.ordinal = evidence.ordinal
         )
         ORDER BY ordinal LIMIT ?",
    )
    .bind(run_id.as_str())
    .bind(i64::try_from(MAX_ACCEPTANCE_CRITERIA).map_err(|_| StatefulRunStoreError::CountOverflow)?)
    .fetch_all(&mut *connection)
    .await?;
    let mut latest = std::collections::BTreeMap::new();
    for stored in evidence {
        latest.insert(stored.ordinal, decode_evidence(stored)?);
    }
    let mut decoded = Vec::with_capacity(criteria.len());
    for stored in criteria {
        let span = match (stored.span_start, stored.span_end) {
            (Some(start), Some(end)) => Some(RequestSpan {
                start: usize::try_from(start).map_err(|_| StatefulRunStoreError::CorruptCount)?,
                end: usize::try_from(end).map_err(|_| StatefulRunStoreError::CorruptCount)?,
            }),
            (None, None) => None,
            _ => return Err(StatefulRunStoreError::CorruptCount),
        };
        decoded.push(AcceptanceCriterion {
            ordinal: u32::try_from(stored.ordinal)
                .map_err(|_| StatefulRunStoreError::CorruptCount)?,
            origin: parse_origin(&stored.origin)?,
            kind: parse_kind(&stored.kind)?,
            state: parse_state(&stored.state)?,
            statement: stored.statement,
            request_span: span,
            artifacts: serde_json::from_str(&stored.artifacts_json)?,
            check_command: stored.check_command,
            note: stored.note,
            revision: u64::try_from(stored.revision)
                .map_err(|_| StatefulRunStoreError::CorruptCount)?,
            evidence: latest.remove(&stored.ordinal),
        });
    }
    Ok(AcceptanceLedger {
        run_id: run_id.clone(),
        revision: u64::try_from(stored.revision)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        workspace_generation: u64::try_from(stored.workspace_generation)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        omission_checked: stored.omission_checked == 1,
        stalled_completions: u32::try_from(stored.stalled_completions)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        criteria: decoded,
    })
}

fn decode_evidence(stored: StoredEvidence) -> Result<AcceptanceEvidence, StatefulRunStoreError> {
    Ok(AcceptanceEvidence {
        sequence: u64::try_from(stored.sequence)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        source: parse_source(&stored.source)?,
        outcome: parse_outcome(&stored.outcome)?,
        command: stored.command,
        exit_code: stored
            .exit_code
            .map(i32::try_from)
            .transpose()
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        output_tail: stored.output_tail,
        output_digest: stored.output_digest,
        artifact_digest: stored.artifact_digest,
        detail: stored.detail,
        workspace_generation: u64::try_from(stored.workspace_generation)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        criterion_revision: u64::try_from(stored.criterion_revision)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        source_id: stored.source_id,
        observed_at_ms: stored.observed_at_ms,
    })
}

macro_rules! codec {
    ($name:ident, $parse:ident, $type:ty, {$($variant:path => $value:literal),+ $(,)?}) => {
        fn $name(value: $type) -> &'static str {
            match value { $($variant => $value,)+ }
        }
        fn $parse(value: &str) -> Result<$type, StatefulRunStoreError> {
            match value {
                $($value => Ok($variant),)+
                _ => Err(StatefulRunStoreError::CorruptEnum(value.to_string())),
            }
        }
    };
}

codec!(origin_name, parse_origin, AcceptanceOrigin, {
    AcceptanceOrigin::User => "user", AcceptanceOrigin::Derived => "derived",
    AcceptanceOrigin::Omission => "omission",
});
codec!(kind_name, parse_kind, AcceptanceKind, {
    AcceptanceKind::Deliverable => "deliverable", AcceptanceKind::Constraint => "constraint",
    AcceptanceKind::Check => "check", AcceptanceKind::Manual => "manual",
});
codec!(state_name, parse_state, AcceptanceState, {
    AcceptanceState::Active => "active", AcceptanceState::Proposed => "proposed",
    AcceptanceState::Dismissed => "dismissed", AcceptanceState::Retired => "retired",
});
codec!(source_name, parse_source, EvidenceSource, {
    EvidenceSource::HostCommand => "hostCommand", EvidenceSource::Manual => "manual",
    EvidenceSource::NoCheck => "noCheck",
});
codec!(outcome_name, parse_outcome, EvidenceOutcome, {
    EvidenceOutcome::Passed => "passed", EvidenceOutcome::Failed => "failed",
    EvidenceOutcome::Observed => "observed", EvidenceOutcome::Unavailable => "unavailable",
});

#[cfg(test)]
#[path = "acceptance_storage_tests.rs"]
mod tests;
