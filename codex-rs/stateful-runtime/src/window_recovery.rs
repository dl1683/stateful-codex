//! Recovery of window publications: closed windows whose staging failed, idle threads whose
//! open suffix a later session publishes, and pending publications paged past failures.

use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::WindowPublication;
use crate::storage::unix_timestamp_millis;
use crate::storage::validate_list_limit;
use crate::window_journal::StoredPublication;
use crate::window_journal::parse_publication;
use crate::window_journal::parse_seq;
use crate::window_journal::to_i64;
use crate::window_journal::validate_identity;

/// Why staging a recovery publication did not happen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdleStaging {
    Staged,
    /// The thread recorded something after it was selected as idle: its window is live.
    BecameActive,
}

impl StatefulRunStore {
    /// Records that a window of `thread_id` for `project_id` closed through `through_seq`.
    pub async fn record_window_closure(
        &self,
        thread_id: &str,
        project_id: &str,
        through_seq: u64,
    ) -> Result<(), StatefulRunStoreError> {
        if through_seq == 0 {
            return Ok(());
        }
        validate_identity(thread_id)?;
        validate_identity(project_id)?;
        sqlx::query(
            "INSERT INTO stateful_window_closures (thread_id, project_id, through_seq, created_at_ms)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (thread_id, project_id, through_seq) DO NOTHING",
        )
        .bind(thread_id)
        .bind(project_id)
        .bind(to_i64(through_seq)?)
        .bind(unix_timestamp_millis()?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The latest closure of `thread_id` for `project_id` (0 when none).
    pub async fn latest_window_closure(
        &self,
        thread_id: &str,
        project_id: &str,
    ) -> Result<u64, StatefulRunStoreError> {
        let seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(through_seq), 0) FROM stateful_window_closures
             WHERE thread_id = ? AND project_id = ?",
        )
        .bind(thread_id)
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        parse_seq(seq)
    }

    /// Stages `publication` only if, under the write lock, the thread still has no observation
    /// for its project after `publication.through_seq` and none since `idle_since_ms`.
    pub async fn stage_idle_window_publication(
        &self,
        publication: &WindowPublication,
        idle_since_ms: i64,
    ) -> Result<IdleStaging, StatefulRunStoreError> {
        for identity in [
            &publication.thread_id,
            &publication.project_id,
            &publication.entry_id,
        ] {
            validate_identity(identity)?;
        }
        // The idleness check and the insertion share one write transaction, which appends also
        // take, so a thread that became active cannot have its live suffix staged.
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (latest_seq, latest_at) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COALESCE(MAX(seq), 0), COALESCE(MAX(created_at_ms), 0)
             FROM stateful_window_events WHERE thread_id = ? AND project_id = ?",
        )
        .bind(&publication.thread_id)
        .bind(&publication.project_id)
        .fetch_one(&mut *transaction)
        .await?;
        let watermark = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(through_seq), 0) FROM stateful_window_publications
             WHERE thread_id = ? AND project_id = ?",
        )
        .bind(&publication.thread_id)
        .bind(&publication.project_id)
        .fetch_one(&mut *transaction)
        .await?;
        if parse_seq(latest_seq)? != publication.through_seq
            || latest_at >= idle_since_ms
            || parse_seq(watermark)? != publication.from_seq
            || publication.through_seq <= publication.from_seq
        {
            transaction.commit().await?;
            return Ok(IdleStaging::BecameActive);
        }
        sqlx::query(
            "INSERT INTO stateful_window_publications (
                thread_id, from_seq, through_seq, project_id, entry_id, content, state,
                created_at_ms, published_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, NULL)",
        )
        .bind(&publication.thread_id)
        .bind(to_i64(publication.from_seq)?)
        .bind(to_i64(publication.through_seq)?)
        .bind(&publication.project_id)
        .bind(&publication.entry_id)
        .bind(&publication.content)
        .bind(unix_timestamp_millis()?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(IdleStaging::Staged)
    }

    /// Pending publications of `project_id` after `after_row` (0 to start), oldest first, with
    /// the row cursor of each, so recovery can move past rows that keep failing.
    pub async fn pending_window_publications_after(
        &self,
        project_id: &str,
        after_row: i64,
        max_results: u32,
    ) -> Result<Vec<(i64, WindowPublication)>, StatefulRunStoreError> {
        validate_list_limit(max_results)?;
        let rows = sqlx::query_as::<_, (i64, String, i64, i64, String, String, String, String)>(
            "SELECT rowid, thread_id, from_seq, through_seq, project_id, entry_id, content, state
             FROM stateful_window_publications
             WHERE project_id = ? AND state = 'pending' AND rowid > ?
             ORDER BY rowid LIMIT ?",
        )
        .bind(project_id)
        .bind(after_row)
        .bind(i64::from(max_results))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(
                |(row, thread_id, from_seq, through_seq, project_id, entry_id, content, state)| {
                    Ok((
                        row,
                        parse_publication(StoredPublication {
                            thread_id,
                            from_seq,
                            through_seq,
                            project_id,
                            entry_id,
                            content,
                            state,
                        })?,
                    ))
                },
            )
            .collect()
    }
}

#[cfg(test)]
#[path = "window_recovery_tests.rs"]
mod tests;
