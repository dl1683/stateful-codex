use serde::Deserialize;
use serde::Serialize;
use sqlx::FromRow;

use crate::NewStatefulTurnMeasurement;
use crate::StatefulMeasurementSummary;
use crate::StatefulRunId;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::StatefulTurnMeasurement;
use crate::StatefulTurnMeasurementPage;
use crate::StatefulTurnStatus;
use crate::StatefulTurnTerminalMeasurement;
use crate::storage::unix_timestamp_millis;
use crate::storage::validate_list_limit;

const MAX_MEASUREMENT_CURSOR_BYTES: usize = 4_096;

impl StatefulRunStore {
    pub async fn record_turn_attribution(
        &self,
        value: NewStatefulTurnMeasurement,
    ) -> Result<StatefulTurnMeasurement, StatefulRunStoreError> {
        validate_measurement(&value)?;
        let run = self
            .get_run(&value.run_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(value.run_id.to_string()))?;
        if run.value.project_id != value.project_id {
            return Err(StatefulRunStoreError::ProjectMismatch);
        }
        if !run.value.thread_ids.contains(&value.thread_id) {
            return Err(StatefulRunStoreError::MeasurementThreadMismatch);
        }

        let now = unix_timestamp_millis()?;
        let rows = sqlx::query(
            "INSERT INTO stateful_turn_measurements (
                run_id, project_id, thread_id, turn_id, status, duration_ms,
                attribution_counters_json, trajectory_json, token_usage_json, completed_at_ms,
                created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?, ?)
             ON CONFLICT DO NOTHING",
        )
        .bind(value.run_id.as_str())
        .bind(&value.project_id)
        .bind(&value.thread_id)
        .bind(&value.turn_id)
        .bind(status_name(value.status))
        .bind(i64::try_from(value.duration_ms).map_err(|_| StatefulRunStoreError::CountOverflow)?)
        .bind(serde_json::to_string(&value.attribution_counters)?)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected();
        let stored = self
            .get_turn_measurement(&value.run_id, &value.turn_id)
            .await?
            .ok_or(StatefulRunStoreError::MeasurementIdentityConflict)?;
        let same_attribution = stored.value.run_id == value.run_id
            && stored.value.project_id == value.project_id
            && stored.value.thread_id == value.thread_id
            && stored.value.turn_id == value.turn_id
            && stored.value.duration_ms == value.duration_ms
            && stored.value.attribution_counters == value.attribution_counters
            && (stored.value.status == value.status || stored.trajectory.is_some());
        if rows == 1 || same_attribution {
            return Ok(stored);
        }
        Err(StatefulRunStoreError::MeasurementIdentityConflict)
    }

    pub async fn record_turn_terminal(
        &self,
        run_id: &StatefulRunId,
        turn_id: &str,
        terminal: StatefulTurnTerminalMeasurement,
    ) -> Result<StatefulTurnMeasurement, StatefulRunStoreError> {
        validate_measurement_id(turn_id)?;
        let measurement = self
            .get_turn_measurement(run_id, turn_id)
            .await?
            .ok_or(StatefulRunStoreError::MeasurementNotFound)?;
        self.record_turn_terminal_for_thread(&measurement.value.thread_id, turn_id, terminal)
            .await
    }

    pub async fn record_turn_terminal_for_thread(
        &self,
        thread_id: &str,
        turn_id: &str,
        terminal: StatefulTurnTerminalMeasurement,
    ) -> Result<StatefulTurnMeasurement, StatefulRunStoreError> {
        validate_measurement_id(thread_id)?;
        validate_measurement_id(turn_id)?;
        if terminal.completed_at_ms.is_some_and(|value| value < 0) {
            return Err(StatefulRunStoreError::InvalidMeasurementTimestamp);
        }
        validate_token_usage(terminal.token_usage.as_ref())?;
        let trajectory_json = serde_json::to_string(&terminal.trajectory)?;
        let token_usage_json = terminal
            .token_usage
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let requested_completed_at_ms = terminal.completed_at_ms;
        let completed_at_ms = requested_completed_at_ms.unwrap_or(unix_timestamp_millis()?);
        let rows = sqlx::query(
            "UPDATE stateful_turn_measurements
             SET status = ?, trajectory_json = ?, token_usage_json = ?, completed_at_ms = ?, updated_at_ms = ?
             WHERE thread_id = ? AND turn_id = ? AND trajectory_json IS NULL",
        )
        .bind(status_name(terminal.status))
        .bind(&trajectory_json)
        .bind(&token_usage_json)
        .bind(completed_at_ms)
        .bind(unix_timestamp_millis()?)
        .bind(thread_id)
        .bind(turn_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        let stored = self
            .get_turn_measurement_for_thread(thread_id, turn_id)
            .await?
            .ok_or(StatefulRunStoreError::MeasurementNotFound)?;
        if rows == 1
            || (stored.value.status == terminal.status
                && stored.trajectory.as_ref() == Some(&terminal.trajectory)
                && stored.token_usage == terminal.token_usage
                && (requested_completed_at_ms.is_none()
                    || stored.completed_at_ms == Some(completed_at_ms)))
        {
            return Ok(stored);
        }
        Err(StatefulRunStoreError::MeasurementIdentityConflict)
    }

    pub async fn get_turn_measurement(
        &self,
        run_id: &StatefulRunId,
        turn_id: &str,
    ) -> Result<Option<StatefulTurnMeasurement>, StatefulRunStoreError> {
        validate_measurement_id(turn_id)?;
        let stored = sqlx::query_as::<_, StoredTurnMeasurement>(
            "SELECT * FROM stateful_turn_measurements WHERE run_id = ? AND turn_id = ?",
        )
        .bind(run_id.as_str())
        .bind(turn_id)
        .fetch_optional(&self.pool)
        .await?;
        stored.map(parse_measurement).transpose()
    }

    pub async fn recent_project_measurements(
        &self,
        project_id: &str,
        max_results: u32,
    ) -> Result<Vec<StatefulTurnMeasurement>, StatefulRunStoreError> {
        Ok(self
            .list_project_measurements(project_id, None, max_results)
            .await?
            .data)
    }

    pub async fn summarize_project_measurements(
        &self,
        project_id: &str,
        max_results: u32,
    ) -> Result<StatefulMeasurementSummary, StatefulRunStoreError> {
        let page = self
            .list_project_measurements(project_id, None, max_results)
            .await?;
        Ok(StatefulMeasurementSummary::from_page(project_id, page))
    }

    pub async fn list_project_measurements(
        &self,
        project_id: &str,
        after: Option<&str>,
        max_results: u32,
    ) -> Result<StatefulTurnMeasurementPage, StatefulRunStoreError> {
        crate::run::validate_project_id(project_id)?;
        validate_list_limit(max_results)?;
        let cursor = after
            .map(|value| parse_measurement_cursor(project_id, value))
            .transpose()?;
        let query_limit = i64::from(max_results) + 1;
        let stored = match cursor {
            Some(cursor) => {
                sqlx::query_as::<_, StoredTurnMeasurement>(
                    "SELECT * FROM stateful_turn_measurements
                     WHERE project_id = ? AND (
                         created_at_ms < ? OR
                         (created_at_ms = ? AND run_id < ?) OR
                         (created_at_ms = ? AND run_id = ? AND turn_id < ?)
                     )
                     ORDER BY created_at_ms DESC, run_id DESC, turn_id DESC LIMIT ?",
                )
                .bind(project_id)
                .bind(cursor.created_at_ms)
                .bind(cursor.created_at_ms)
                .bind(&cursor.run_id)
                .bind(cursor.created_at_ms)
                .bind(&cursor.run_id)
                .bind(&cursor.turn_id)
                .bind(query_limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, StoredTurnMeasurement>(
                    "SELECT * FROM stateful_turn_measurements
                     WHERE project_id = ?
                     ORDER BY created_at_ms DESC, run_id DESC, turn_id DESC LIMIT ?",
                )
                .bind(project_id)
                .bind(query_limit)
                .fetch_all(&self.pool)
                .await?
            }
        };
        let mut data = stored
            .into_iter()
            .map(parse_measurement)
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if data.len() > max_results as usize {
            data.truncate(max_results as usize);
            data.last().map(measurement_cursor).transpose()?
        } else {
            None
        };
        Ok(StatefulTurnMeasurementPage { data, next_cursor })
    }
}

impl StatefulRunStore {
    async fn get_turn_measurement_for_thread(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Option<StatefulTurnMeasurement>, StatefulRunStoreError> {
        let stored = sqlx::query_as::<_, StoredTurnMeasurement>(
            "SELECT * FROM stateful_turn_measurements WHERE thread_id = ? AND turn_id = ?",
        )
        .bind(thread_id)
        .bind(turn_id)
        .fetch_optional(&self.pool)
        .await?;
        stored.map(parse_measurement).transpose()
    }
}

#[derive(FromRow)]
struct StoredTurnMeasurement {
    run_id: String,
    project_id: String,
    thread_id: String,
    turn_id: String,
    status: String,
    duration_ms: i64,
    attribution_counters_json: String,
    trajectory_json: Option<String>,
    token_usage_json: Option<String>,
    completed_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MeasurementCursor {
    project_id: String,
    created_at_ms: i64,
    run_id: String,
    turn_id: String,
}

fn parse_measurement_cursor(
    project_id: &str,
    value: &str,
) -> Result<MeasurementCursor, StatefulRunStoreError> {
    if value.len() > MAX_MEASUREMENT_CURSOR_BYTES {
        return Err(StatefulRunStoreError::InvalidListCursor);
    }
    let cursor: MeasurementCursor =
        serde_json::from_str(value).map_err(|_| StatefulRunStoreError::InvalidListCursor)?;
    if cursor.project_id != project_id
        || cursor.created_at_ms < 0
        || validate_measurement_id(&cursor.run_id).is_err()
        || validate_measurement_id(&cursor.turn_id).is_err()
    {
        return Err(StatefulRunStoreError::InvalidListCursor);
    }
    Ok(cursor)
}

fn measurement_cursor(
    measurement: &StatefulTurnMeasurement,
) -> Result<String, StatefulRunStoreError> {
    Ok(serde_json::to_string(&MeasurementCursor {
        project_id: measurement.value.project_id.clone(),
        created_at_ms: measurement.created_at_ms,
        run_id: measurement.value.run_id.to_string(),
        turn_id: measurement.value.turn_id.clone(),
    })?)
}

fn parse_measurement(
    stored: StoredTurnMeasurement,
) -> Result<StatefulTurnMeasurement, StatefulRunStoreError> {
    let token_usage = stored
        .token_usage_json
        .map(|value| serde_json::from_str(&value))
        .transpose()?;
    validate_token_usage(token_usage.as_ref())?;
    Ok(StatefulTurnMeasurement {
        value: NewStatefulTurnMeasurement {
            run_id: StatefulRunId::parse(stored.run_id)?,
            project_id: stored.project_id,
            thread_id: stored.thread_id,
            turn_id: stored.turn_id,
            status: parse_status(&stored.status)?,
            duration_ms: u64::try_from(stored.duration_ms)
                .map_err(|_| StatefulRunStoreError::CorruptCount)?,
            attribution_counters: serde_json::from_str(&stored.attribution_counters_json)?,
        },
        trajectory: stored
            .trajectory_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        token_usage,
        completed_at_ms: stored.completed_at_ms,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: stored.updated_at_ms,
    })
}

fn validate_token_usage(
    value: Option<&crate::StatefulTokenUsage>,
) -> Result<(), StatefulRunStoreError> {
    let Some(value) = value else {
        return Ok(());
    };
    if [
        value.total_tokens,
        value.input_tokens,
        value.cached_input_tokens,
        value.cache_write_input_tokens,
        value.output_tokens,
        value.reasoning_output_tokens,
    ]
    .into_iter()
    .any(|count| count < 0)
    {
        return Err(StatefulRunStoreError::InvalidMeasurementCount);
    }
    Ok(())
}

fn validate_measurement(value: &NewStatefulTurnMeasurement) -> Result<(), StatefulRunStoreError> {
    crate::run::validate_project_id(&value.project_id)?;
    validate_measurement_id(&value.thread_id)?;
    validate_measurement_id(&value.turn_id)
}

fn validate_measurement_id(value: &str) -> Result<(), StatefulRunStoreError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(StatefulRunStoreError::InvalidRecordId);
    }
    Ok(())
}

fn status_name(status: StatefulTurnStatus) -> &'static str {
    match status {
        StatefulTurnStatus::Completed => "completed",
        StatefulTurnStatus::Failed => "failed",
        StatefulTurnStatus::Aborted => "aborted",
    }
}

fn parse_status(value: &str) -> Result<StatefulTurnStatus, StatefulRunStoreError> {
    match value {
        "completed" => Ok(StatefulTurnStatus::Completed),
        "failed" => Ok(StatefulTurnStatus::Failed),
        "aborted" => Ok(StatefulTurnStatus::Aborted),
        _ => Err(StatefulRunStoreError::CorruptEnum(value.to_string())),
    }
}
