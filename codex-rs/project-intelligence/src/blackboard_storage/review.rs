//! Paged listing of a project's current knowledge for the user to review, in a fixed order:
//! rules first (the user's own before others), then decisions, then everything else.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;

impl BlackboardStore {
    /// Active entries of the project starting at `offset` in review order, at most `limit`,
    /// and whether more follow. Within a group, the most recently changed come first.
    pub async fn active_review_page(
        &self,
        project_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<BlackboardEntry>, bool), BlackboardStoreError> {
        let mut transaction = self.pool.begin().await?;
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
             ORDER BY CASE
                 WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user' THEN 0
                 WHEN revision.kind = 'instruction' THEN 1
                 WHEN revision.kind = 'decision' THEN 2
                 ELSE 3 END,
               entry.updated_at_ms DESC, entry.id
             LIMIT ? OFFSET ?",
        )
        .bind(project_id)
        .bind(i64::from(limit) + 1)
        .bind(i64::from(offset))
        .fetch_all(&mut *transaction)
        .await?;
        let more = ids.len() > limit as usize;
        let mut entries = Vec::with_capacity(ids.len().min(limit as usize));
        for id in ids.into_iter().take(limit as usize) {
            let id = BlackboardEntryId::parse(id)?;
            if let Some(entry) = load_entry(&mut transaction, project_id, &id).await? {
                entries.push(entry);
            }
        }
        transaction.commit().await?;
        Ok((entries, more))
    }
}
