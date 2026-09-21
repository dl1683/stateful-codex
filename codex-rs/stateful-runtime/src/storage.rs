use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_state::SqliteConfig;
use sqlx::FromRow;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::NewObligation;
use crate::NewStatefulRun;
use crate::ObligationPacket;
use crate::RunBudget;
use crate::StatefulObligation;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunUpdate;
use crate::WorkflowMode;
use crate::run::StatefulRunError;

const DATABASE_NAME: &str = "stateful_runtime_1.sqlite";
const INITIAL_REVISION: i64 = 1;
const MAX_LEASE_DURATION_MS: u32 = 10 * 60 * 1_000;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutonomousClaimRequest {
    pub owner_id: String,
    pub previous_turn_id: String,
    pub lease_duration_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutonomousClaimOutcome {
    Claimed {
        run: StatefulRun,
        lease_expires_at_ms: i64,
    },
    AlreadyClaimed,
    Leased,
    NotEligible,
    BudgetExhausted(StatefulRun),
}

#[derive(Clone)]
pub struct StatefulRunStore {
    pub(crate) pool: SqlitePool,
}

impl StatefulRunStore {
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, StatefulRunStoreError> {
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
            .await?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool })
    }

    pub async fn create_run(
        &self,
        id: StatefulRunId,
        value: NewStatefulRun,
    ) -> Result<StatefulRun, StatefulRunStoreError> {
        value.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_run(&mut transaction, &id).await? {
            if existing.value != value {
                return Err(StatefulRunStoreError::RunIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
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
        .bind(INITIAL_REVISION)
        .bind(now)
        .bind(now)
        .bind(i64::from(value.budget.max_continuations))
        .bind(i64::from(value.budget.max_elapsed_seconds))
        .execute(&mut *transaction)
        .await?;
        for (position, thread_id) in value.thread_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO stateful_run_threads (run_id, position, thread_id)
                 VALUES (?, ?, ?)",
            )
            .bind(id.as_str())
            .bind(i64::try_from(position).map_err(|_| StatefulRunStoreError::CountOverflow)?)
            .bind(thread_id)
            .execute(&mut *transaction)
            .await?;
        }
        let run = load_run(&mut transaction, &id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(run)
    }

    pub async fn get_run(
        &self,
        id: &StatefulRunId,
    ) -> Result<Option<StatefulRun>, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_run(&mut connection, id).await
    }

    pub async fn run_for_thread(
        &self,
        thread_id: &str,
    ) -> Result<Option<StatefulRun>, StatefulRunStoreError> {
        let raw_id = sqlx::query_scalar::<_, String>(
            "SELECT run.id FROM stateful_runs AS run
             JOIN stateful_run_threads AS thread ON thread.run_id = run.id
             WHERE thread.thread_id = ?
               AND run.status NOT IN ('completed', 'cancelled', 'failed')
             ORDER BY run.updated_at_ms DESC, run.id LIMIT 1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(raw_id) = raw_id else {
            return Ok(None);
        };
        self.get_run(&StatefulRunId::parse(raw_id)?).await
    }

    pub async fn update_run(
        &self,
        id: &StatefulRunId,
        update: StatefulRunUpdate,
    ) -> Result<StatefulRun, StatefulRunStoreError> {
        update.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_run(&mut transaction, id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
        if current.revision != update.expected_revision {
            return Err(StatefulRunStoreError::RevisionConflict {
                expected: update.expected_revision,
                actual: current.revision,
            });
        }
        if !valid_transition(current.status, update.status) {
            return Err(StatefulRunStoreError::InvalidTransition {
                from: current.status,
                to: update.status,
            });
        }
        let expected_revision = i64::try_from(update.expected_revision)
            .map_err(|_| StatefulRunStoreError::CountOverflow)?;
        let strategy_changed = current.strategy != update.strategy;
        let rows = sqlx::query(
            "UPDATE stateful_runs
             SET status = ?, strategy = ?,
                 strategy_revision = strategy_revision + ?, result = ?,
                 revision = revision + 1, updated_at_ms = ?
             WHERE id = ? AND revision = ?",
        )
        .bind(status_name(update.status))
        .bind(update.strategy)
        .bind(i64::from(strategy_changed))
        .bind(update.result)
        .bind(unix_timestamp_millis()?)
        .bind(id.as_str())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if rows != 1 {
            return Err(StatefulRunStoreError::ConcurrentMutation);
        }
        let run = load_run(&mut transaction, id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(run)
    }

    pub async fn claim_autonomous_continuation(
        &self,
        id: &StatefulRunId,
        request: AutonomousClaimRequest,
    ) -> Result<AutonomousClaimOutcome, StatefulRunStoreError> {
        validate_record_id(&request.owner_id)?;
        validate_record_id(&request.previous_turn_id)?;
        if request.lease_duration_ms == 0 || request.lease_duration_ms > MAX_LEASE_DURATION_MS {
            return Err(StatefulRunStoreError::InvalidLeaseDuration);
        }

        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_run(&mut transaction, id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
        if current.value.mode != WorkflowMode::Autonomous
            || current.status != StatefulRunStatus::Running
        {
            return Ok(AutonomousClaimOutcome::NotEligible);
        }

        let now = unix_timestamp_millis()?;
        let already_claimed = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM stateful_run_continuations
             WHERE run_id = ? AND previous_turn_id = ?",
        )
        .bind(id.as_str())
        .bind(&request.previous_turn_id)
        .fetch_optional(&mut *transaction)
        .await?
        .is_some();
        if already_claimed {
            transaction.commit().await?;
            return Ok(AutonomousClaimOutcome::AlreadyClaimed);
        }

        let elapsed_ms = now.saturating_sub(current.created_at_ms);
        let maximum_elapsed_ms = i64::from(current.value.budget.max_elapsed_seconds) * 1_000;
        if current.continuations_used >= current.value.budget.max_continuations
            || elapsed_ms >= maximum_elapsed_ms
        {
            sqlx::query(
                "UPDATE stateful_runs
                 SET status = 'blocked', revision = revision + 1, updated_at_ms = ?
                 WHERE id = ?",
            )
            .bind(now)
            .bind(id.as_str())
            .execute(&mut *transaction)
            .await?;
            let blocked = load_run(&mut transaction, id)
                .await?
                .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
            transaction.commit().await?;
            return Ok(AutonomousClaimOutcome::BudgetExhausted(blocked));
        }

        let lease = sqlx::query_as::<_, StoredLease>(
            "SELECT owner_id, lease_expires_at_ms FROM stateful_run_leases WHERE run_id = ?",
        )
        .bind(id.as_str())
        .fetch_optional(&mut *transaction)
        .await?;
        if lease.is_some_and(|lease| {
            lease.owner_id != request.owner_id && lease.lease_expires_at_ms > now
        }) {
            transaction.commit().await?;
            return Ok(AutonomousClaimOutcome::Leased);
        }

        let lease_expires_at_ms = now
            .checked_add(i64::from(request.lease_duration_ms))
            .ok_or(StatefulRunStoreError::TimestampOverflow)?;
        sqlx::query(
            "INSERT INTO stateful_run_continuations (run_id, previous_turn_id, claimed_at_ms)
             VALUES (?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&request.previous_turn_id)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO stateful_run_leases (
                run_id, owner_id, lease_expires_at_ms, previous_turn_id
             ) VALUES (?, ?, ?, ?)
             ON CONFLICT(run_id) DO UPDATE SET
                owner_id = excluded.owner_id,
                lease_expires_at_ms = excluded.lease_expires_at_ms,
                previous_turn_id = excluded.previous_turn_id",
        )
        .bind(id.as_str())
        .bind(&request.owner_id)
        .bind(lease_expires_at_ms)
        .bind(&request.previous_turn_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE stateful_runs
             SET continuations_used = continuations_used + 1,
                 revision = revision + 1, updated_at_ms = ?
             WHERE id = ?",
        )
        .bind(now)
        .bind(id.as_str())
        .execute(&mut *transaction)
        .await?;
        let run = load_run(&mut transaction, id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(AutonomousClaimOutcome::Claimed {
            run,
            lease_expires_at_ms,
        })
    }

    pub async fn append_obligation(
        &self,
        id: String,
        value: NewObligation,
    ) -> Result<StatefulObligation, StatefulRunStoreError> {
        validate_record_id(&id)?;
        value.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_obligation_by_id(&mut transaction, &id).await? {
            if existing.value != value {
                return Err(StatefulRunStoreError::ObligationIdentityConflict(id));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        let run = load_run(&mut transaction, &value.run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(value.run_id.to_string()))?;
        if run.value.project_id != value.project_id {
            return Err(StatefulRunStoreError::ProjectMismatch);
        }
        let result = sqlx::query(
            "INSERT INTO stateful_obligations (
                id, project_id, run_id, packet_json, provenance_source_id,
                revision, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&value.project_id)
        .bind(value.run_id.as_str())
        .bind(serde_json::to_string(&value.packet)?)
        .bind(&value.provenance_source_id)
        .bind(INITIAL_REVISION)
        .bind(unix_timestamp_millis()?)
        .execute(&mut *transaction)
        .await?;
        let sequence = result.last_insert_rowid();
        let obligation = load_obligation_by_sequence(&mut transaction, sequence)
            .await?
            .ok_or_else(|| StatefulRunStoreError::ObligationNotFound(id))?;
        transaction.commit().await?;
        Ok(obligation)
    }

    pub async fn latest_obligation(
        &self,
        run_id: &StatefulRunId,
    ) -> Result<Option<StatefulObligation>, StatefulRunStoreError> {
        let sequence = sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM stateful_obligations
             WHERE run_id = ? ORDER BY sequence DESC LIMIT 1",
        )
        .bind(run_id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(sequence) = sequence else {
            return Ok(None);
        };
        let mut connection = self.pool.acquire().await?;
        load_obligation_by_sequence(&mut connection, sequence).await
    }

    pub async fn list_obligations(
        &self,
        run_id: &StatefulRunId,
        after_sequence: Option<u64>,
        max_results: u32,
    ) -> Result<Vec<StatefulObligation>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let after_sequence = after_sequence
            .map(i64::try_from)
            .transpose()
            .map_err(|_| StatefulRunStoreError::CountOverflow)?
            .unwrap_or_default();
        let sequences = sqlx::query_scalar::<_, i64>(
            "SELECT sequence FROM stateful_obligations
             WHERE run_id = ? AND sequence > ? ORDER BY sequence LIMIT ?",
        )
        .bind(run_id.as_str())
        .bind(after_sequence)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?;
        let mut connection = self.pool.acquire().await?;
        let mut data = Vec::with_capacity(sequences.len());
        for sequence in sequences {
            data.push(
                load_obligation_by_sequence(&mut connection, sequence)
                    .await?
                    .ok_or(StatefulRunStoreError::CorruptObligationSequence(sequence))?,
            );
        }
        Ok(data)
    }
}

#[derive(FromRow)]
struct StoredRun {
    id: String,
    project_id: String,
    goal: String,
    mode: String,
    status: String,
    strategy: Option<String>,
    strategy_revision: i64,
    result: Option<String>,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    max_continuations: i64,
    max_elapsed_seconds: i64,
    continuations_used: i64,
}

#[derive(FromRow)]
struct StoredLease {
    owner_id: String,
    lease_expires_at_ms: i64,
}

#[derive(FromRow)]
struct StoredObligation {
    sequence: i64,
    id: String,
    project_id: String,
    run_id: String,
    packet_json: String,
    provenance_source_id: String,
    revision: i64,
    created_at_ms: i64,
}

pub(crate) async fn load_run(
    connection: &mut SqliteConnection,
    id: &StatefulRunId,
) -> Result<Option<StatefulRun>, StatefulRunStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredRun>("SELECT * FROM stateful_runs WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(&mut *connection)
        .await?
    else {
        return Ok(None);
    };
    let thread_ids = sqlx::query_scalar::<_, String>(
        "SELECT thread_id FROM stateful_run_threads WHERE run_id = ? ORDER BY position",
    )
    .bind(id.as_str())
    .fetch_all(connection)
    .await?;
    let value = NewStatefulRun {
        project_id: stored.project_id,
        thread_ids,
        goal: stored.goal,
        mode: parse_mode(&stored.mode)?,
        budget: RunBudget {
            max_continuations: parse_u32(stored.max_continuations)?,
            max_elapsed_seconds: parse_u32(stored.max_elapsed_seconds)?,
        },
    };
    value.validate()?;
    Ok(Some(StatefulRun {
        id: StatefulRunId::parse(stored.id)?,
        value,
        status: parse_status(&stored.status)?,
        strategy: stored.strategy,
        strategy_revision: parse_count(stored.strategy_revision)?,
        result: stored.result,
        continuations_used: parse_u32(stored.continuations_used)?,
        revision: parse_count(stored.revision)?,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: stored.updated_at_ms,
    }))
}

async fn load_obligation_by_id(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Option<StatefulObligation>, StatefulRunStoreError> {
    let sequence =
        sqlx::query_scalar::<_, i64>("SELECT sequence FROM stateful_obligations WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?;
    match sequence {
        Some(sequence) => load_obligation_by_sequence(connection, sequence).await,
        None => Ok(None),
    }
}

async fn load_obligation_by_sequence(
    connection: &mut SqliteConnection,
    sequence: i64,
) -> Result<Option<StatefulObligation>, StatefulRunStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredObligation>(
        "SELECT * FROM stateful_obligations WHERE sequence = ?",
    )
    .bind(sequence)
    .fetch_optional(connection)
    .await?
    else {
        return Ok(None);
    };
    let value = NewObligation {
        project_id: stored.project_id,
        run_id: StatefulRunId::parse(stored.run_id)?,
        packet: serde_json::from_str::<ObligationPacket>(&stored.packet_json)?,
        provenance_source_id: stored.provenance_source_id,
    };
    value.validate()?;
    Ok(Some(StatefulObligation {
        id: stored.id,
        value,
        sequence: parse_count(stored.sequence)?,
        revision: parse_count(stored.revision)?,
        created_at_ms: stored.created_at_ms,
    }))
}

fn valid_transition(from: StatefulRunStatus, to: StatefulRunStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (StatefulRunStatus::Pending, StatefulRunStatus::Running)
                | (StatefulRunStatus::Pending, StatefulRunStatus::Cancelled)
                | (StatefulRunStatus::Pending, StatefulRunStatus::Failed)
                | (StatefulRunStatus::Running, StatefulRunStatus::Paused)
                | (StatefulRunStatus::Running, StatefulRunStatus::Completed)
                | (StatefulRunStatus::Running, StatefulRunStatus::Cancelled)
                | (StatefulRunStatus::Running, StatefulRunStatus::Blocked)
                | (StatefulRunStatus::Running, StatefulRunStatus::Failed)
                | (StatefulRunStatus::Paused, StatefulRunStatus::Running)
                | (StatefulRunStatus::Paused, StatefulRunStatus::Cancelled)
                | (StatefulRunStatus::Blocked, StatefulRunStatus::Running)
                | (StatefulRunStatus::Blocked, StatefulRunStatus::Cancelled)
                | (StatefulRunStatus::Blocked, StatefulRunStatus::Failed)
        )
}

macro_rules! enum_codec {
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

enum_codec!(mode_name, parse_mode, WorkflowMode, {
    WorkflowMode::Autonomous => "autonomous", WorkflowMode::Collaborative => "collaborative",
    WorkflowMode::Socratic => "socratic",
});
enum_codec!(status_name, parse_status, StatefulRunStatus, {
    StatefulRunStatus::Pending => "pending", StatefulRunStatus::Running => "running",
    StatefulRunStatus::Paused => "paused", StatefulRunStatus::Completed => "completed",
    StatefulRunStatus::Cancelled => "cancelled", StatefulRunStatus::Blocked => "blocked",
    StatefulRunStatus::Failed => "failed",
});

fn parse_count(value: i64) -> Result<u64, StatefulRunStoreError> {
    u64::try_from(value).map_err(|_| StatefulRunStoreError::CorruptCount)
}

fn parse_u32(value: i64) -> Result<u32, StatefulRunStoreError> {
    u32::try_from(value).map_err(|_| StatefulRunStoreError::CorruptCount)
}

fn validate_record_id(value: &str) -> Result<(), StatefulRunStoreError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(StatefulRunStoreError::InvalidRecordId);
    }
    Ok(())
}

pub(crate) fn validate_list_limit(max_results: u32) -> Result<(), StatefulRunStoreError> {
    if max_results == 0 || max_results > 101 {
        return Err(StatefulRunStoreError::InvalidListLimit);
    }
    Ok(())
}

pub(crate) fn unix_timestamp_millis() -> Result<i64, StatefulRunStoreError> {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH)?;
    i64::try_from(duration.as_millis()).map_err(|_| StatefulRunStoreError::TimestampOverflow)
}

#[derive(Debug, Error)]
pub enum StatefulRunStoreError {
    #[error(transparent)]
    InvalidRun(#[from] StatefulRunError),
    #[error(transparent)]
    InvalidSteering(crate::steering::SteeringError),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Time(#[from] std::time::SystemTimeError),
    #[error("run not found: {0}")]
    RunNotFound(String),
    #[error("run ID was already used for different content: {0}")]
    RunIdentityConflict(String),
    #[error("run revision conflict: expected {expected}, found {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("invalid run status transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: StatefulRunStatus,
        to: StatefulRunStatus,
    },
    #[error("run changed during a guarded update")]
    ConcurrentMutation,
    #[error("obligation ID must be non-empty, bounded, and contain no controls")]
    InvalidRecordId,
    #[error("obligation ID was already used for different content: {0}")]
    ObligationIdentityConflict(String),
    #[error("obligation not found: {0}")]
    ObligationNotFound(String),
    #[error("stored obligation sequence has no record: {0}")]
    CorruptObligationSequence(i64),
    #[error("obligation project does not match its run")]
    ProjectMismatch,
    #[error("steering ID was already used for different content: {0}")]
    SteeringIdentityConflict(String),
    #[error("steering instruction not found: {0}")]
    SteeringNotFound(String),
    #[error("invalid steering transition from {from:?} to {to:?}")]
    InvalidSteeringTransition {
        from: crate::SteeringStatus,
        to: crate::SteeringStatus,
    },
    #[error("applied steering strategy revision does not match the run")]
    StrategyRevisionMismatch,
    #[error("applied steering must change the current run strategy")]
    SteeringStrategyUnchanged,
    #[error("steering cannot be applied while the run is {0:?}")]
    SteeringRunNotExecutable(StatefulRunStatus),
    #[error("list limit must be between 1 and 101")]
    InvalidListLimit,
    #[error("autonomous lease duration must be between 1 and 600000 milliseconds")]
    InvalidLeaseDuration,
    #[error("list cursor does not belong to the requested run")]
    InvalidListCursor,
    #[error("stored runtime enum value is unknown: {0}")]
    CorruptEnum(String),
    #[error("stored runtime count is invalid")]
    CorruptCount,
    #[error("runtime count overflow")]
    CountOverflow,
    #[error("runtime timestamp overflow")]
    TimestampOverflow,
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
