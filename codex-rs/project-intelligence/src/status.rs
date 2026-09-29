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
    pub last_refresh: Option<ProjectRefreshStatus>,
    pub updated_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRefreshStatus {
    pub inventory_complete: bool,
    pub region_coverage_complete: bool,
    pub files_indexed: u64,
    pub regions_indexed: u64,
    pub files_skipped: u64,
    pub missing_files: u64,
    pub truncated: bool,
    pub scan_duration_ms: u64,
    pub publication_duration_ms: u64,
    pub completed_at_ms: i64,
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
    refresh_inventory_complete: Option<bool>,
    refresh_region_coverage_complete: Option<bool>,
    refresh_files_indexed: Option<i64>,
    refresh_regions_indexed: Option<i64>,
    refresh_files_skipped: Option<i64>,
    refresh_missing_files: Option<i64>,
    refresh_truncated: Option<bool>,
    refresh_scan_duration_ms: Option<i64>,
    refresh_publication_duration_ms: Option<i64>,
    refresh_completed_at_ms: Option<i64>,
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
                (SELECT inventory_complete FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_inventory_complete,
                (SELECT region_coverage_complete FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_region_coverage_complete,
                (SELECT files_indexed FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_files_indexed,
                (SELECT regions_indexed FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_regions_indexed,
                (SELECT files_skipped FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_files_skipped,
                (SELECT missing_files FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_missing_files,
                (SELECT truncated FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_truncated,
                (SELECT scan_duration_ms FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_scan_duration_ms,
                (SELECT publication_duration_ms FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_publication_duration_ms,
                (SELECT completed_at_ms FROM project_index_refresh_status
                    WHERE project_id = ?) AS refresh_completed_at_ms,
                (SELECT MAX(updated_at_ms) FROM (
                    SELECT updated_at_ms FROM hierarchy_nodes WHERE project_id = ?
                    UNION ALL SELECT updated_at_ms FROM context_map_entries WHERE project_id = ?
                    UNION ALL SELECT updated_at_ms FROM blackboard_entries WHERE project_id = ?
                    UNION ALL SELECT completed_at_ms FROM project_index_refresh_status
                        WHERE project_id = ?
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
        .bind(project_id)
        .fetch_one(self.database.pool())
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
            last_refresh: match (
                stored.refresh_inventory_complete,
                stored.refresh_region_coverage_complete,
                stored.refresh_files_indexed,
                stored.refresh_regions_indexed,
                stored.refresh_files_skipped,
                stored.refresh_missing_files,
                stored.refresh_truncated,
                stored.refresh_scan_duration_ms,
                stored.refresh_publication_duration_ms,
                stored.refresh_completed_at_ms,
            ) {
                (
                    Some(inventory_complete),
                    Some(region_coverage_complete),
                    Some(files_indexed),
                    Some(regions_indexed),
                    Some(files_skipped),
                    Some(missing_files),
                    Some(truncated),
                    Some(scan_duration_ms),
                    Some(publication_duration_ms),
                    Some(completed_at_ms),
                ) => Some(ProjectRefreshStatus {
                    inventory_complete,
                    region_coverage_complete,
                    files_indexed: count(files_indexed)?,
                    regions_indexed: count(regions_indexed)?,
                    files_skipped: count(files_skipped)?,
                    missing_files: count(missing_files)?,
                    truncated,
                    scan_duration_ms: count(scan_duration_ms)?,
                    publication_duration_ms: count(publication_duration_ms)?,
                    completed_at_ms,
                }),
                (None, None, None, None, None, None, None, None, None, None) => None,
                _ => return Err(HierarchyStoreError::CorruptCount),
            },
            updated_at_ms: stored.updated_at_ms,
        })
    }
}
