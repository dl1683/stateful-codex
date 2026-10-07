//! Whether a project has any context-map entries at all, to tell a never-indexed project
//! from one whose routes are region-only, missing, or replaced.

use crate::ContextMapStore;
use crate::ContextMapStoreError;

impl ContextMapStore {
    /// Whether any context-map entry, in any lifecycle, exists for `project_id`.
    pub async fn has_entries(&self, project_id: &str) -> Result<bool, ContextMapStoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM context_map_entries WHERE project_id = ?)",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }
}

#[cfg(test)]
#[path = "context_map_presence_tests.rs"]
mod tests;
