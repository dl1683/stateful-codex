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
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::acceptance::AcceptanceError;
use crate::acceptance::MAX_ACCEPTANCE_CHANGES;
use crate::acceptance::MAX_ACCEPTANCE_CRITERIA;
use crate::acceptance::MAX_OUTPUT_TAIL_BYTES;
use crate::acceptance::unmet_criteria;
use crate::acceptance_changes::ChangeContext;
use crate::acceptance_changes::apply_change;
use crate::acceptance_changes::insert_omission_proposal;
use crate::acceptance_changes::next_ordinal;
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
            let context = ChangeContext {
                run: &run,
                ledger: &ledger,
                ledger_revision: expected_revision + 1,
                source_id,
                now,
            };
            apply_change(&mut transaction, &context, change).await?;
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
        let context = ChangeContext {
            run: &run,
            ledger: &ledger,
            ledger_revision: ledger.revision + 1,
            source_id: "host-omission-check",
            now,
        };
        let mut added = 0;
        for (ordinal, (statement, span)) in
            (next_ordinal(&ledger)..).zip(proposals.into_iter().take(slots))
        {
            insert_omission_proposal(&mut transaction, &context, ordinal, &statement, span).await?;
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

    /// Starts a completion verification attempt and leases it to `owner`. The run's public
    /// status stays `Running`; a second attempt waits until the lease ends or expires.
    pub async fn begin_verification(
        &self,
        run_id: &StatefulRunId,
        owner: &str,
        lease_ms: u32,
    ) -> Result<u64, StatefulRunStoreError> {
        crate::run::validate_source_id(owner)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        ensure_ledger(&mut transaction, run_id).await?;
        let now = unix_timestamp_millis()?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        if let Some(expires) = ledger.verification_lease_expires_at_ms
            && expires > now
        {
            return Err(AcceptanceError::VerificationLeased(expires).into());
        }
        let expires = now
            .checked_add(i64::from(lease_ms))
            .ok_or(StatefulRunStoreError::TimestampOverflow)?;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET verification_attempt = verification_attempt + 1, verification_owner = ?,
                 verification_lease_expires_at_ms = ?, updated_at_ms = ?
             WHERE run_id = ?",
        )
        .bind(owner)
        .bind(expires)
        .bind(now)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger.verification_attempt)
    }

    /// Ends a verification attempt without completing (a refusal); only its owner can end it.
    pub async fn end_verification(
        &self,
        run_id: &StatefulRunId,
        owner: &str,
        attempt: u64,
    ) -> Result<(), StatefulRunStoreError> {
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers
             SET verification_owner = NULL, verification_lease_expires_at_ms = NULL
             WHERE run_id = ? AND verification_owner = ? AND verification_attempt = ?",
        )
        .bind(run_id.as_str())
        .bind(owner)
        .bind(i64::try_from(attempt).map_err(|_| StatefulRunStoreError::CountOverflow)?)
        .execute(&self.pool)
        .await?;
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
            if let Some((owner, attempt)) = &commit.verification {
                consume_verification_lease(connection, run_id, owner, *attempt).await?;
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

/// The terminal transaction consumes the attempt it was validated in; an expired or replaced
/// lease means another attempt may have observed different state.
async fn consume_verification_lease(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
    owner: &str,
    attempt: u64,
) -> Result<(), StatefulRunStoreError> {
    let consumed = sqlx::query(
        "UPDATE stateful_acceptance_ledgers
         SET verification_owner = NULL, verification_lease_expires_at_ms = NULL
         WHERE run_id = ? AND verification_owner = ? AND verification_attempt = ?
           AND verification_lease_expires_at_ms > ?",
    )
    .bind(run_id.as_str())
    .bind(owner)
    .bind(i64::try_from(attempt).map_err(|_| StatefulRunStoreError::CountOverflow)?)
    .bind(unix_timestamp_millis()?)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if consumed != 1 {
        return Err(StatefulRunStoreError::AcceptanceChanged);
    }
    Ok(())
}

pub(crate) fn executes(status: StatefulRunStatus) -> bool {
    matches!(
        status,
        StatefulRunStatus::Running | StatefulRunStatus::Blocked
    )
}

pub(crate) struct EvidenceRow<'a> {
    pub(crate) source: EvidenceSource,
    pub(crate) outcome: EvidenceOutcome,
    pub(crate) command: Option<&'a str>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) output_tail: Option<&'a str>,
    pub(crate) output_digest: Option<&'a str>,
    pub(crate) artifact_digest: Option<&'a str>,
    pub(crate) detail: Option<&'a str>,
    pub(crate) workspace_generation: u64,
    pub(crate) source_id: &'a str,
}

pub(crate) async fn insert_evidence(
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

pub(crate) async fn host_checked(
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
            stalled_at_revision, verification_attempt, updated_at_ms
         ) VALUES (?, 0, 0, 0, 0, 0, 0, ?)",
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
    verification_attempt: i64,
    verification_lease_expires_at_ms: Option<i64>,
}

#[derive(FromRow)]
struct StoredCriterion {
    ordinal: i64,
    criterion_id: String,
    origin: String,
    kind: String,
    state: String,
    statement: String,
    requirement: String,
    required: bool,
    depends_on_json: String,
    milestone: Option<String>,
    expected_observation: Option<String>,
    span_start: Option<i64>,
    span_end: Option<i64>,
    artifacts_json: String,
    check_command: Option<String>,
    note: Option<String>,
    revision: i64,
    ledger_revision: i64,
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
        "SELECT revision, workspace_generation, omission_checked, stalled_completions,
                verification_attempt, verification_lease_expires_at_ms
         FROM stateful_acceptance_ledgers WHERE run_id = ?",
    )
    .bind(run_id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(AcceptanceLedger::empty(run_id.clone()));
    };
    let criteria = sqlx::query_as::<_, StoredCriterion>(
        "SELECT ordinal, criterion_id, origin, kind, state, statement, requirement, required,
                depends_on_json, milestone, expected_observation, span_start, span_end,
                artifacts_json, check_command, note, revision, ledger_revision
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
            id: stored.criterion_id,
            ordinal: u32::try_from(stored.ordinal)
                .map_err(|_| StatefulRunStoreError::CorruptCount)?,
            origin: parse_origin(&stored.origin)?,
            kind: parse_kind(&stored.kind)?,
            state: parse_state(&stored.state)?,
            statement: stored.statement,
            requirement: stored.requirement,
            required: stored.required,
            depends_on: serde_json::from_str(&stored.depends_on_json)?,
            milestone: stored.milestone,
            request_span: span,
            artifacts: serde_json::from_str(&stored.artifacts_json)?,
            check_command: stored.check_command,
            expected_observation: stored.expected_observation,
            note: stored.note,
            revision: u64::try_from(stored.revision)
                .map_err(|_| StatefulRunStoreError::CorruptCount)?,
            ledger_revision: u64::try_from(stored.ledger_revision)
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
        verification_attempt: u64::try_from(stored.verification_attempt)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        verification_lease_expires_at_ms: stored.verification_lease_expires_at_ms,
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
        pub(crate) fn $name(value: $type) -> &'static str {
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
