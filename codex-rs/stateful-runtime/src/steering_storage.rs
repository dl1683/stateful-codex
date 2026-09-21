use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::NewSteeringInstruction;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::StatefulSteering;
use crate::SteeringId;
use crate::SteeringStatus;
use crate::SteeringUpdate;
use crate::steering::SteeringError;
use crate::storage::unix_timestamp_millis;
use crate::storage::validate_list_limit;

impl StatefulRunStore {
    pub async fn get_steering(
        &self,
        id: &SteeringId,
    ) -> Result<Option<StatefulSteering>, StatefulRunStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_steering(&mut connection, id).await
    }

    pub async fn submit_steering(
        &self,
        id: SteeringId,
        value: NewSteeringInstruction,
    ) -> Result<StatefulSteering, StatefulRunStoreError> {
        value.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_steering(&mut transaction, &id).await? {
            if existing.value != value {
                return Err(StatefulRunStoreError::SteeringIdentityConflict(
                    id.to_string(),
                ));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        let run_project: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM stateful_runs WHERE id = ?")
                .bind(value.run_id.as_str())
                .fetch_optional(&mut *transaction)
                .await?;
        if run_project.as_deref() != Some(&value.project_id) {
            return Err(StatefulRunStoreError::ProjectMismatch);
        }
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO stateful_steering (
                id, project_id, run_id, input, affected_obligation_ids_json,
                status, resulting_strategy_revision, reason, revision,
                created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, 'submitted', NULL, NULL, 1, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&value.project_id)
        .bind(value.run_id.as_str())
        .bind(&value.input)
        .bind(serde_json::to_string(&value.affected_obligation_ids)?)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let steering = load_steering(&mut transaction, &id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::SteeringNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(steering)
    }

    pub async fn list_steering(
        &self,
        run_id: &StatefulRunId,
        after: Option<&SteeringId>,
        max_results: u32,
    ) -> Result<Vec<StatefulSteering>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let cursor = match after {
            Some(id) => {
                let mut connection = self.pool.acquire().await?;
                let steering = load_steering(&mut connection, id)
                    .await?
                    .ok_or_else(|| StatefulRunStoreError::SteeringNotFound(id.to_string()))?;
                if &steering.value.run_id != run_id {
                    return Err(StatefulRunStoreError::InvalidListCursor);
                }
                Some((steering.created_at_ms, steering.id.to_string()))
            }
            None => None,
        };
        let (after_created_at_ms, after_id) = cursor
            .map(|(created_at_ms, id)| (Some(created_at_ms), Some(id)))
            .unwrap_or((None, None));
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT id FROM stateful_steering WHERE run_id = ?
               AND (? IS NULL OR created_at_ms > ? OR (created_at_ms = ? AND id > ?))
             ORDER BY created_at_ms, id LIMIT ?",
        )
        .bind(run_id.as_str())
        .bind(after_created_at_ms)
        .bind(after_created_at_ms)
        .bind(after_created_at_ms)
        .bind(after_id)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?;
        let mut connection = self.pool.acquire().await?;
        let mut data = Vec::with_capacity(ids.len());
        for raw_id in ids {
            let id = SteeringId::parse(raw_id)?;
            data.push(
                load_steering(&mut connection, &id)
                    .await?
                    .ok_or_else(|| StatefulRunStoreError::SteeringNotFound(id.to_string()))?,
            );
        }
        Ok(data)
    }

    pub async fn update_steering(
        &self,
        id: &SteeringId,
        update: SteeringUpdate,
    ) -> Result<StatefulSteering, StatefulRunStoreError> {
        update.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_steering(&mut transaction, id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::SteeringNotFound(id.to_string()))?;
        if current.revision != update.expected_revision {
            return Err(StatefulRunStoreError::RevisionConflict {
                expected: update.expected_revision,
                actual: current.revision,
            });
        }
        if !valid_transition(current.status, update.status) {
            return Err(StatefulRunStoreError::InvalidSteeringTransition {
                from: current.status,
                to: update.status,
            });
        }
        if let Some(strategy_revision) = update.resulting_strategy_revision {
            let current_strategy: Option<i64> =
                sqlx::query_scalar("SELECT strategy_revision FROM stateful_runs WHERE id = ?")
                    .bind(current.value.run_id.as_str())
                    .fetch_optional(&mut *transaction)
                    .await?;
            if current_strategy.and_then(|value| u64::try_from(value).ok())
                != Some(strategy_revision)
            {
                return Err(StatefulRunStoreError::StrategyRevisionMismatch);
            }
        }
        let rows = sqlx::query(
            "UPDATE stateful_steering
             SET status = ?, resulting_strategy_revision = ?, reason = ?,
                 revision = revision + 1, updated_at_ms = ?
             WHERE id = ? AND revision = ?",
        )
        .bind(status_name(update.status))
        .bind(
            update
                .resulting_strategy_revision
                .map(i64::try_from)
                .transpose()
                .map_err(|_| StatefulRunStoreError::CountOverflow)?,
        )
        .bind(update.reason)
        .bind(unix_timestamp_millis()?)
        .bind(id.as_str())
        .bind(
            i64::try_from(update.expected_revision)
                .map_err(|_| StatefulRunStoreError::CountOverflow)?,
        )
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if rows != 1 {
            return Err(StatefulRunStoreError::ConcurrentMutation);
        }
        let steering = load_steering(&mut transaction, id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::SteeringNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(steering)
    }
}

#[derive(FromRow)]
struct StoredSteering {
    id: String,
    project_id: String,
    run_id: String,
    input: String,
    affected_obligation_ids_json: String,
    status: String,
    resulting_strategy_revision: Option<i64>,
    reason: Option<String>,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

async fn load_steering(
    connection: &mut SqliteConnection,
    id: &SteeringId,
) -> Result<Option<StatefulSteering>, StatefulRunStoreError> {
    let Some(stored) =
        sqlx::query_as::<_, StoredSteering>("SELECT * FROM stateful_steering WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(connection)
            .await?
    else {
        return Ok(None);
    };
    let value = NewSteeringInstruction {
        project_id: stored.project_id,
        run_id: StatefulRunId::parse(stored.run_id)?,
        input: stored.input,
        affected_obligation_ids: serde_json::from_str(&stored.affected_obligation_ids_json)?,
    };
    value.validate()?;
    Ok(Some(StatefulSteering {
        id: SteeringId::parse(stored.id)?,
        value,
        status: parse_status(&stored.status)?,
        resulting_strategy_revision: stored
            .resulting_strategy_revision
            .map(u64::try_from)
            .transpose()
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        reason: stored.reason,
        revision: u64::try_from(stored.revision)
            .map_err(|_| StatefulRunStoreError::CorruptCount)?,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: stored.updated_at_ms,
    }))
}

fn valid_transition(from: SteeringStatus, to: SteeringStatus) -> bool {
    from == to
        || matches!(
            (from, to),
            (SteeringStatus::Submitted, SteeringStatus::Acknowledged)
                | (SteeringStatus::Submitted, SteeringStatus::Rejected)
                | (SteeringStatus::Acknowledged, SteeringStatus::Applied)
                | (SteeringStatus::Acknowledged, SteeringStatus::Rejected)
        )
}

fn status_name(value: SteeringStatus) -> &'static str {
    match value {
        SteeringStatus::Submitted => "submitted",
        SteeringStatus::Acknowledged => "acknowledged",
        SteeringStatus::Applied => "applied",
        SteeringStatus::Rejected => "rejected",
    }
}

fn parse_status(value: &str) -> Result<SteeringStatus, StatefulRunStoreError> {
    match value {
        "submitted" => Ok(SteeringStatus::Submitted),
        "acknowledged" => Ok(SteeringStatus::Acknowledged),
        "applied" => Ok(SteeringStatus::Applied),
        "rejected" => Ok(SteeringStatus::Rejected),
        _ => Err(StatefulRunStoreError::CorruptEnum(value.to_string())),
    }
}

impl From<SteeringError> for StatefulRunStoreError {
    fn from(error: SteeringError) -> Self {
        Self::InvalidSteering(error)
    }
}
