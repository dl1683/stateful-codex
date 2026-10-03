//! Source order of user messages against retirements, by the memory-change journal rather
//! than clocks: a message stands after every change journaled before it was first captured,
//! and an entry was retired at the journal row that retired it.

use crate::BlackboardEntry;

use super::BlackboardStore;
use super::BlackboardStoreError;

impl BlackboardStore {
    /// The journal position of a user message: the sequence before the first change journaled
    /// for its turn, or, when it has none yet, the latest sequence now. A retried capture or a
    /// cold resume of the same message keeps the position its first capture had.
    pub async fn message_watermark(
        &self,
        project_id: &str,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<u64, BlackboardStoreError> {
        let sequence = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(
                 (SELECT MIN(sequence) - 1 FROM memory_changes
                  WHERE project_id = ? AND thread_id = ? AND turn_id = ?),
                 (SELECT COALESCE(MAX(sequence), 0) FROM memory_changes WHERE project_id = ?))",
        )
        .bind(project_id)
        .bind(thread_id)
        .bind(turn_id)
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        u64::try_from(sequence).map_err(|_| BlackboardStoreError::RevisionOverflow)
    }

    /// The journal sequence at which `entry` was retired: the save of its successor when it
    /// was replaced, its forgetting or invalidation otherwise. None when the retirement was
    /// not journaled (an older entry).
    pub async fn retirement_sequence(
        &self,
        entry: &BlackboardEntry,
    ) -> Result<Option<u64>, BlackboardStoreError> {
        let project_id = &entry.value.project_id;
        let sequence = match &entry.superseded_by {
            Some(successor) => sqlx::query_scalar::<_, Option<i64>>(
                "SELECT MIN(sequence) FROM memory_changes WHERE project_id = ? AND entry_id = ?",
            )
            .bind(project_id)
            .bind(successor.as_str()),
            None => sqlx::query_scalar::<_, Option<i64>>(
                "SELECT MAX(sequence) FROM memory_changes
                 WHERE project_id = ? AND entry_id = ?
                   AND operation IN ('forgotten', 'invalidated')",
            )
            .bind(project_id)
            .bind(entry.id.as_str()),
        }
        .fetch_one(&self.pool)
        .await?;
        sequence
            .map(u64::try_from)
            .transpose()
            .map_err(|_| BlackboardStoreError::RevisionOverflow)
    }
}
