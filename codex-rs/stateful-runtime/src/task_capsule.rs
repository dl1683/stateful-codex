//! The task capsule of each continuation window, captured once so every step of the window,
//! a retry and a same-thread resume render exactly the same capsule.

use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::unix_timestamp_millis;

/// Bound of one stored capsule body.
pub const MAX_TASK_CAPSULE_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCapsule {
    /// The journal sequence the capsule was built through.
    pub through_seq: u64,
    pub body: String,
}

impl StatefulRunStore {
    /// Stores a window's capsule unless one exists and returns the stored capsule.
    pub async fn capture_task_capsule(
        &self,
        thread_id: &str,
        window_id: &str,
        capsule: &TaskCapsule,
    ) -> Result<TaskCapsule, StatefulRunStoreError> {
        if thread_id.is_empty()
            || window_id.is_empty()
            || capsule.body.is_empty()
            || capsule.body.len() > MAX_TASK_CAPSULE_BYTES
        {
            return Err(StatefulRunStoreError::InvalidRecordId);
        }
        sqlx::query(
            "INSERT INTO stateful_task_capsules (
                thread_id, window_id, through_seq, body, created_at_ms
             ) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (thread_id, window_id) DO NOTHING",
        )
        .bind(thread_id)
        .bind(window_id)
        .bind(i64::try_from(capsule.through_seq).map_err(|_| StatefulRunStoreError::CountOverflow)?)
        .bind(&capsule.body)
        .bind(unix_timestamp_millis()?)
        .execute(&self.pool)
        .await?;
        self.task_capsule(thread_id, window_id)
            .await?
            .ok_or_else(|| StatefulRunStoreError::RunNotFound(window_id.to_string()))
    }

    pub async fn task_capsule(
        &self,
        thread_id: &str,
        window_id: &str,
    ) -> Result<Option<TaskCapsule>, StatefulRunStoreError> {
        let row = sqlx::query_as::<_, (i64, String)>(
            "SELECT through_seq, body FROM stateful_task_capsules
             WHERE thread_id = ? AND window_id = ?",
        )
        .bind(thread_id)
        .bind(window_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(through_seq, body)| {
            Ok(TaskCapsule {
                through_seq: u64::try_from(through_seq)
                    .map_err(|_| StatefulRunStoreError::CorruptCount)?,
                body,
            })
        })
        .transpose()
    }

    /// Whether the thread journaled an edit after `after_seq`.
    pub async fn edited_after(
        &self,
        thread_id: &str,
        after_seq: u64,
    ) -> Result<bool, StatefulRunStoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS (
                SELECT 1 FROM stateful_window_events
                WHERE thread_id = ? AND seq > ? AND kind = 'edit'
             )",
        )
        .bind(thread_id)
        .bind(i64::try_from(after_seq).map_err(|_| StatefulRunStoreError::CountOverflow)?)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }
}

#[cfg(test)]
#[path = "task_capsule_tests.rs"]
mod tests;
