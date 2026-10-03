//! The render decision for each context window of a thread, made once when the window opens.

use sqlx::FromRow;

use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::storage::unix_timestamp_millis;

const MAX_IDENTITY_BYTES: usize = 128;

/// How Stateful renders a context window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextWindowMode {
    /// The full project, run and continuity packets.
    Full,
    /// Rules, a memory pointer and the task capsule beside a native compaction summary.
    Continuation,
}

/// Why a context window opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextWindowReason {
    /// The thread's first window, or the first one Stateful rendered in it.
    ThreadStart,
    /// Native compaction: a summary and the retained user messages carry the conversation.
    Compaction,
    /// History was replaced without a summary.
    Reset,
    /// No boundary was observed, such as a thread written before windows were recorded.
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextWindowDecision {
    pub thread_id: String,
    /// The selected project the decision is for: selecting another project in the same
    /// thread opens that project's own startup, never this decision.
    pub project_id: String,
    pub window_id: String,
    pub window_number: u64,
    pub mode: ContextWindowMode,
    pub reason: ContextWindowReason,
}

#[derive(FromRow)]
struct StoredWindow {
    thread_id: String,
    project_id: String,
    window_id: String,
    window_number: i64,
    mode: String,
    reason: String,
}

impl StatefulRunStore {
    /// Records the decision for a window unless one exists, and returns the stored decision:
    /// the first decision for a window is final, so a retry or a later step never changes it.
    pub async fn decide_context_window(
        &self,
        decision: &ContextWindowDecision,
    ) -> Result<ContextWindowDecision, StatefulRunStoreError> {
        for identity in [
            &decision.thread_id,
            &decision.project_id,
            &decision.window_id,
        ] {
            if identity.is_empty()
                || identity.len() > MAX_IDENTITY_BYTES
                || identity.chars().any(char::is_control)
            {
                return Err(StatefulRunStoreError::InvalidRecordId);
            }
        }
        let window_number = i64::try_from(decision.window_number)
            .map_err(|_| StatefulRunStoreError::CountOverflow)?;
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO stateful_context_windows (
                thread_id, project_id, window_id, window_number, mode, reason, created_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (thread_id, project_id, window_id) DO NOTHING",
        )
        .bind(&decision.thread_id)
        .bind(&decision.project_id)
        .bind(&decision.window_id)
        .bind(window_number)
        .bind(mode_name(decision.mode))
        .bind(reason_name(decision.reason))
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.context_window(
            &decision.thread_id,
            &decision.project_id,
            &decision.window_id,
        )
        .await?
        .ok_or_else(|| StatefulRunStoreError::RunNotFound(decision.window_id.clone()))
    }

    pub async fn context_window(
        &self,
        thread_id: &str,
        project_id: &str,
        window_id: &str,
    ) -> Result<Option<ContextWindowDecision>, StatefulRunStoreError> {
        let row = sqlx::query_as::<_, StoredWindow>(
            "SELECT thread_id, project_id, window_id, window_number, mode, reason
             FROM stateful_context_windows
             WHERE thread_id = ? AND project_id = ? AND window_id = ?",
        )
        .bind(thread_id)
        .bind(project_id)
        .bind(window_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ContextWindowDecision {
                thread_id: row.thread_id,
                project_id: row.project_id,
                window_id: row.window_id,
                window_number: u64::try_from(row.window_number)
                    .map_err(|_| StatefulRunStoreError::CorruptCount)?,
                mode: parse_mode(&row.mode)?,
                reason: parse_reason(&row.reason)?,
            })
        })
        .transpose()
    }

    /// Whether this thread already opened a window for this project. A window it has not
    /// decided yet was then opened inside the thread (a compaction whose initial context is
    /// injected later), not by a fork or a project switch, which start without any record.
    pub async fn thread_has_context_windows(
        &self,
        thread_id: &str,
        project_id: &str,
    ) -> Result<bool, StatefulRunStoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS (
                SELECT 1 FROM stateful_context_windows WHERE thread_id = ? AND project_id = ?
             )",
        )
        .bind(thread_id)
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }
}

fn mode_name(mode: ContextWindowMode) -> &'static str {
    match mode {
        ContextWindowMode::Full => "full",
        ContextWindowMode::Continuation => "continuation",
    }
}

fn parse_mode(value: &str) -> Result<ContextWindowMode, StatefulRunStoreError> {
    match value {
        "full" => Ok(ContextWindowMode::Full),
        "continuation" => Ok(ContextWindowMode::Continuation),
        _ => Err(StatefulRunStoreError::CorruptEnum(value.to_string())),
    }
}

fn reason_name(reason: ContextWindowReason) -> &'static str {
    match reason {
        ContextWindowReason::ThreadStart => "threadStart",
        ContextWindowReason::Compaction => "compaction",
        ContextWindowReason::Reset => "reset",
        ContextWindowReason::Unknown => "unknown",
    }
}

fn parse_reason(value: &str) -> Result<ContextWindowReason, StatefulRunStoreError> {
    match value {
        "threadStart" => Ok(ContextWindowReason::ThreadStart),
        "compaction" => Ok(ContextWindowReason::Compaction),
        "reset" => Ok(ContextWindowReason::Reset),
        "unknown" => Ok(ContextWindowReason::Unknown),
        _ => Err(StatefulRunStoreError::CorruptEnum(value.to_string())),
    }
}

#[cfg(test)]
#[path = "context_window_tests.rs"]
mod tests;
