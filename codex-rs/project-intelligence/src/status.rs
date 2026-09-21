use sqlx::FromRow;

use crate::HierarchyStore;
use crate::storage::HierarchyStoreError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIntelligenceStatus {
    pub initialized: bool,
    pub revision: u64,
    pub hierarchy_node_count: u64,
    pub file_count: u64,
    pub missing_source_count: u64,
    pub context_map_entry_count: u64,
    pub blackboard_entry_count: u64,
    pub promoted_entry_count: u64,
    pub updated_at_ms: Option<i64>,
}

#[derive(FromRow)]
struct StoredStatus {
    hierarchy_node_count: i64,
    file_count: i64,
    missing_source_count: i64,
    context_map_entry_count: i64,
    blackboard_entry_count: i64,
    promoted_entry_count: i64,
    revision: i64,
    updated_at_ms: Option<i64>,
}

impl HierarchyStore {
    pub async fn project_intelligence_status(
        &self,
        project_id: &str,
    ) -> Result<ProjectIntelligenceStatus, HierarchyStoreError> {
        let stored = sqlx::query_as::<_, StoredStatus>(
            "SELECT
                (SELECT COUNT(*) FROM hierarchy_nodes WHERE project_id = ?) AS hierarchy_node_count,
                (SELECT COUNT(*) FROM hierarchy_nodes
                    WHERE project_id = ? AND kind = 'file' AND lifecycle = 'active') AS file_count,
                (SELECT COUNT(*) FROM hierarchy_nodes
                    WHERE project_id = ? AND lifecycle = 'missing') AS missing_source_count,
                (SELECT COUNT(*) FROM context_map_entries WHERE project_id = ?)
                    AS context_map_entry_count,
                (SELECT COUNT(*) FROM blackboard_entries AS entry
                    JOIN blackboard_entry_revisions AS current
                      ON current.entry_id = entry.id AND current.revision = entry.revision
                    WHERE entry.project_id = ? AND current.state = 'active')
                    AS blackboard_entry_count,
                (SELECT COUNT(*) FROM blackboard_entries AS entry
                    JOIN blackboard_entry_revisions AS current
                      ON current.entry_id = entry.id AND current.revision = entry.revision
                    WHERE entry.project_id = ? AND current.state = 'active'
                      AND current.root_promotion = 'promoted') AS promoted_entry_count,
                COALESCE((SELECT revision FROM project_intelligence_revisions
                    WHERE project_id = ?), 0) AS revision,
                (SELECT MAX(updated_at_ms) FROM (
                    SELECT updated_at_ms FROM hierarchy_nodes WHERE project_id = ?
                    UNION ALL SELECT updated_at_ms FROM context_map_entries WHERE project_id = ?
                    UNION ALL SELECT updated_at_ms FROM blackboard_entries WHERE project_id = ?
                )) AS updated_at_ms",
        )
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?;
        let count =
            |value: i64| u64::try_from(value).map_err(|_| HierarchyStoreError::CorruptCount);
        let hierarchy_node_count = count(stored.hierarchy_node_count)?;
        Ok(ProjectIntelligenceStatus {
            initialized: hierarchy_node_count > 0,
            revision: count(stored.revision)?,
            hierarchy_node_count,
            file_count: count(stored.file_count)?,
            missing_source_count: count(stored.missing_source_count)?,
            context_map_entry_count: count(stored.context_map_entry_count)?,
            blackboard_entry_count: count(stored.blackboard_entry_count)?,
            promoted_entry_count: count(stored.promoted_entry_count)?,
            updated_at_ms: stored.updated_at_ms,
        })
    }
}
