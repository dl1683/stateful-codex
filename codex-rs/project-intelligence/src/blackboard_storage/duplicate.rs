//! Exact-duplicate lookup for agent-recorded knowledge.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardKind;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::kind_name;
use super::load_entry;

impl BlackboardStore {
    /// The project's active agent-recorded entry of `kind` whose content is exactly
    /// `content`, if any. Recording the same wording again would add no knowledge, so a
    /// writer can treat the record as already present instead of creating a copy.
    pub async fn active_agent_entry_with_content(
        &self,
        project_id: &str,
        kind: BlackboardKind,
        content: &str,
    ) -> Result<Option<BlackboardEntry>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        let id = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
               AND revision.provenance_kind = 'agent'
               AND revision.kind = ? AND revision.content = ?
             ORDER BY entry.created_at_ms, entry.id
             LIMIT 1",
        )
        .bind(project_id)
        .bind(kind_name(kind))
        .bind(content)
        .fetch_optional(&mut *connection)
        .await?;
        let Some(id) = id else {
            return Ok(None);
        };
        let id = BlackboardEntryId::parse(id).map_err(BlackboardStoreError::InvalidEntry)?;
        load_entry(&mut connection, project_id, &id).await
    }
}
