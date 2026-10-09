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
use crate::DismissalReceipt;
use crate::EvidenceOutcome;
use crate::EvidenceSource;
use crate::PlanAdmission;
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
use crate::acceptance::SteeringReconciliation;
use crate::acceptance::unmet_criteria;
use crate::acceptance_changes::ChangeContext;
use crate::acceptance_changes::apply_change;
use crate::acceptance_changes::insert_omission_proposal;
use crate::acceptance_changes::next_ordinal;
use crate::acceptance_coverage::MAX_OMISSION_PROPOSALS;
use crate::acceptance_coverage::uncovered_sentences;
use crate::acceptance_request::acceptance_request;
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
        let request = acceptance_request(&mut transaction, &run).await?;
        let mut changed = false;
        for change in changes {
            let ledger = load_ledger(&mut transaction, run_id).await?;
            let context = ChangeContext {
                run: &run,
                request: &request.text,
                ledger: &ledger,
                ledger_revision: expected_revision + 1,
                source_id,
                now,
            };
            changed |= apply_change(&mut transaction, &context, change).await?;
        }
        // A no-op is not progress: it neither advances the revision nor resets the refusal
        // count.
        if changed {
            bump_revision(&mut transaction, run_id, now).await?;
        }
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok(ledger)
    }

    /// The omission pass: proposes, for review, the next uncovered goal sentences (at most
    /// `MAX_OMISSION_PROPOSALS`, and never more than the ledger has room for). Coverage is
    /// recomputed from durable state, so sentences left over by the cap, by a full ledger or
    /// by a restart stay uncovered and keep gating. Returns the ledger and how many uncovered
    /// sentences remain without a proposal.
    pub async fn propose_uncovered(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<(AcceptanceLedger, usize), StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = load_run(&mut transaction, run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(run_id.to_string()))?;
        if run.status.is_terminal() {
            return Err(StatefulRunStoreError::AcceptanceRunState(run.status));
        }
        let request = acceptance_request(&mut transaction, &run).await?;
        let ledger = load_ledger(&mut transaction, run_id).await?;
        let uncovered = uncovered_sentences(&request.text, &ledger);
        let slots = MAX_ACCEPTANCE_CRITERIA
            .saturating_sub(ledger.criteria.len())
            .min(MAX_OMISSION_PROPOSALS);
        let now = unix_timestamp_millis()?;
        let context = ChangeContext {
            run: &run,
            request: &request.text,
            ledger: &ledger,
            ledger_revision: ledger.revision + 1,
            source_id: "host-omission-check",
            now,
        };
        let mut added = 0;
        for (ordinal, span) in (next_ordinal(&ledger)..).zip(uncovered.iter().take(slots)) {
            let quote = span
                .quote(&request.text)
                .ok_or(AcceptanceError::InvalidSpan)?;
            insert_omission_proposal(
                &mut transaction,
                &context,
                ordinal,
                &proposal_statement(quote),
                *span,
            )
            .await?;
            added += 1;
        }
        if added > 0 {
            bump_revision(&mut transaction, run_id, now).await?;
        }
        let ledger = load_ledger(&mut transaction, run_id).await?;
        transaction.commit().await?;
        Ok((ledger, uncovered.len() - added))
    }

    /// Counts one observed command execution (any kind, read-only included).
    pub async fn record_execution(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<(), StatefulRunStoreError> {
        self.adjust_counters(run_id, "observed_executions = observed_executions + 1")
            .await
    }

    async fn adjust_counters(
        &self,
        run_id: &StatefulRunId,
        assignment: &'static str,
    ) -> Result<(), StatefulRunStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let Some(run) = load_run(&mut transaction, run_id).await? else {
            return Ok(());
        };
        if run.status.is_terminal() {
            return Ok(());
        }
        ensure_ledger(&mut transaction, run_id).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE stateful_acceptance_ledgers SET {assignment}, updated_at_ms = ? WHERE run_id = ?"
        )))
        .bind(unix_timestamp_millis()?)
        .bind(run_id.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
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
            // A mutation observed between the check's start and this insert means the check
            // did not run against one workspace state.
            let raced = observed.start_generation != ledger.workspace_generation;
            let (outcome, detail) = if raced && observed.outcome == EvidenceOutcome::Passed {
                (
                    EvidenceOutcome::Unavailable,
                    Some("the workspace changed while the check ran"),
                )
            } else {
                (observed.outcome, observed.detail.as_deref())
            };
            insert_evidence(
                &mut transaction,
                run_id,
                criterion,
                EvidenceRow {
                    source: EvidenceSource::HostCommand,
                    outcome,
                    command: Some(&observed.command),
                    exit_code: observed.exit_code,
                    output_tail: Some(&bounded_tail(&observed.output_tail)),
                    output_digest: Some(&observed.output_digest),
                    artifact_digest: observed.artifact_digest.as_deref(),
                    checker_digest: observed.checker_digest.as_deref(),
                    detail,
                    workspace_generation: observed.start_generation,
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
}

/// Evaluates the gate inside the terminal transaction. Every `Completed` write needs a host
/// decision (`commit`); the policy (coverage, verdicts, blockers, approvals) is re-derived
/// here from durable state, never taken from the caller.
pub(crate) async fn enforce_acceptance_gate(
    connection: &mut SqliteConnection,
    run: &StatefulRun,
    commit: Option<&AcceptanceCommit>,
) -> Result<(), StatefulRunStoreError> {
    let Some(commit) = commit else {
        return Err(StatefulRunStoreError::CompletionRequiresAcceptanceDecision);
    };
    let ledger = load_ledger(connection, &run.id).await?;
    if commit.ledger_revision != ledger.revision
        || commit.workspace_generation != ledger.workspace_generation
    {
        return Err(StatefulRunStoreError::AcceptanceChanged);
    }
    // Commands whose effects are not accounted for yet could still change or fail the
    // evidence this decision relies on.
    if ledger.pending_commands > 0 {
        return Err(StatefulRunStoreError::AcceptanceGate(format!(
            "{} commands of this run have not finished or have unaccounted effects",
            ledger.pending_commands
        )));
    }
    crate::acceptance_lifecycle::consume_verification_lease(
        connection,
        &run.id,
        &commit.verification.owner,
        commit.verification.attempt,
    )
    .await?;
    let latest = sqlx::query_as::<_, (i64, String)>(
        "SELECT sequence, packet_json FROM stateful_obligations
         WHERE run_id = ? ORDER BY sequence DESC LIMIT 1",
    )
    .bind(run.id.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    let latest_sequence = latest
        .as_ref()
        .map(|(sequence, _)| u64::try_from(*sequence))
        .transpose()
        .map_err(|_| StatefulRunStoreError::CorruptCount)?;
    if latest_sequence != commit.validated_obligation_sequence {
        return Err(StatefulRunStoreError::AcceptanceChanged);
    }
    if let Some((_, packet)) = latest
        && !serde_json::from_str::<crate::ObligationPacket>(&packet)?
            .blockers
            .is_empty()
    {
        return Err(StatefulRunStoreError::AcceptanceGate(
            "the latest recorded obligation declares open blockers".to_string(),
        ));
    }
    let unreconciled = sqlx::query_scalar::<_, String>(
        "SELECT id FROM stateful_steering
         WHERE run_id = ? AND status = 'applied' AND id NOT IN (
             SELECT steering_id FROM stateful_acceptance_steering WHERE run_id = ?
         ) ORDER BY created_at_ms, id LIMIT 1",
    )
    .bind(run.id.as_str())
    .bind(run.id.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(steering_id) = unreconciled {
        return Err(StatefulRunStoreError::AcceptanceGate(format!(
            "applied steering {steering_id} may have changed the scope; reconcile it with stateful_acceptance_update before completing"
        )));
    }
    let mut unmet = unmet_criteria(&ledger, &commit.artifacts, &commit.checkers)
        .into_iter()
        .map(|(ordinal, reason)| format!("C{ordinal}: {reason}"))
        .collect::<Vec<_>>();
    let request = acceptance_request(connection, run).await?;
    let uncovered = uncovered_sentences(&request.text, &ledger).len();
    if uncovered > 0 {
        unmet.push(format!(
            "{uncovered} request sentences are covered by no criterion or proposal"
        ));
    }
    if !unmet.is_empty() {
        return Err(StatefulRunStoreError::AcceptanceGate(unmet.join("; ")));
    }
    // The cheap-lookup admission is recorded, so the completed run shows it owed no coverage.
    if crate::acceptance_coverage::coverage_exempt(&request.text, &ledger) {
        ensure_ledger(connection, &run.id).await?;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers SET exemption = 'cheapLookup' WHERE run_id = ?",
        )
        .bind(run.id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

/// A proposal's statement: the sentence, cut at a character boundary when very long.
fn proposal_statement(quote: &str) -> String {
    const MAX_PROPOSAL_BYTES: usize = 480;
    let quote = quote
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if quote.len() <= MAX_PROPOSAL_BYTES {
        return quote;
    }
    let mut end = MAX_PROPOSAL_BYTES;
    while !quote.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", quote[..end].trim_end())
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
    pub(crate) checker_digest: Option<&'a str>,
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
            artifact_digest, checker_digest, detail, workspace_generation, criterion_revision,
            source_id, observed_at_ms
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .bind(row.checker_digest)
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

pub(crate) async fn ensure_ledger(
    connection: &mut SqliteConnection,
    run_id: &StatefulRunId,
) -> Result<(), StatefulRunStoreError> {
    sqlx::query(
        "INSERT OR IGNORE INTO stateful_acceptance_ledgers (
            run_id, revision, workspace_generation, observed_executions, stalled_completions,
            stalled_fingerprint, verification_attempt, updated_at_ms
         ) VALUES (?, 0, 0, 0, 0, '', 0, ?)",
    )
    .bind(run_id.as_str())
    .bind(unix_timestamp_millis()?)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn bump_revision(
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
    observed_executions: i64,
    stalled_completions: i64,
    verification_attempt: i64,
    verification_lease_expires_at_ms: Option<i64>,
    exemption: Option<String>,
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
    checker_json: String,
    check_command: Option<String>,
    check_cwd: Option<String>,
    plan_criterion_revision: Option<i64>,
    plan_checker_digest: Option<String>,
    plan_ledger_revision: Option<i64>,
    dismissal_covered_by: Option<i64>,
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
    checker_digest: Option<String>,
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
        "SELECT revision, workspace_generation, observed_executions, stalled_completions,
                verification_attempt, verification_lease_expires_at_ms, exemption
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
                artifacts_json, checker_json, check_command, check_cwd, plan_criterion_revision,
                plan_checker_digest, plan_ledger_revision, dismissal_covered_by, note, revision,
                ledger_revision
         FROM stateful_acceptance_criteria WHERE run_id = ? ORDER BY ordinal LIMIT ?",
    )
    .bind(run_id.as_str())
    .bind(i64::try_from(MAX_ACCEPTANCE_CRITERIA).map_err(|_| StatefulRunStoreError::CountOverflow)?)
    .fetch_all(&mut *connection)
    .await?;
    let evidence = sqlx::query_as::<_, StoredEvidence>(
        "SELECT sequence, ordinal, source, outcome, command, exit_code, output_tail,
                output_digest, artifact_digest, checker_digest, detail, workspace_generation,
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
            checker: serde_json::from_str(&stored.checker_json)?,
            check_command: stored.check_command,
            check_cwd: stored.check_cwd,
            expected_observation: stored.expected_observation,
            plan: match (
                stored.plan_criterion_revision,
                stored.plan_checker_digest,
                stored.plan_ledger_revision,
            ) {
                (Some(criterion_revision), Some(checker_digest), Some(ledger_revision)) => {
                    Some(PlanAdmission {
                        criterion_revision: u64::try_from(criterion_revision)
                            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
                        checker_digest,
                        ledger_revision: u64::try_from(ledger_revision)
                            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
                    })
                }
                (None, None, None) => None,
                _ => return Err(StatefulRunStoreError::CorruptCount),
            },
            dismissal: stored
                .dismissal_covered_by
                .map(|covered_by| {
                    u32::try_from(covered_by)
                        .map(DismissalReceipt::CoveredBy)
                        .map_err(|_| StatefulRunStoreError::CorruptCount)
                })
                .transpose()?,
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
        observed_executions: u64::try_from(stored.observed_executions)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        stalled_completions: u32::try_from(stored.stalled_completions)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        pending_commands: u64::try_from(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM stateful_acceptance_pending WHERE run_id = ?",
            )
            .bind(run_id.as_str())
            .fetch_one(&mut *connection)
            .await?,
        )
        .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        reconciled_steering: sqlx::query_as::<_, (String, String)>(
            "SELECT steering_id, reason FROM stateful_acceptance_steering
             WHERE run_id = ? ORDER BY reconciled_at_ms, steering_id LIMIT 64",
        )
        .bind(run_id.as_str())
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(|(steering_id, reason)| SteeringReconciliation {
            steering_id,
            reason,
        })
        .collect(),
        cheap_lookup: stored.exemption.is_some(),
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
        checker_digest: stored.checker_digest,
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
    AcceptanceKind::Existence => "existence",
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
