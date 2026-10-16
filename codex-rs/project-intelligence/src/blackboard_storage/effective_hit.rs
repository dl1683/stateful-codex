//! Exact model reads shared by transaction-owning domain callers.

use sqlx::SqliteConnection;

use crate::BlackboardEntryId;
use crate::BlackboardHit;

use super::BlackboardStoreError;
use super::identity::EntryReader;
use super::identity::entry_source_eligible_on;
use super::query::load_hit;

/// The caller must hold one read transaction across eligibility and materialization.
/// This operation neither starts nor finishes that transaction. Incomplete identity
/// coverage retains the legacy exact-read result (`None`), as do ineligible entries.
/// The full hit loader is intentionally unchanged and is not a bounded wire projection.
pub(super) async fn read_on(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &BlackboardEntryId,
) -> Result<Option<BlackboardHit>, BlackboardStoreError> {
    if !entry_source_eligible_on(connection, project_id, entry_id).await? {
        return Ok(None);
    }
    Ok(Some(
        load_hit(
            connection,
            project_id,
            entry_id.to_string(),
            EntryReader::ModelTool,
        )
        .await?,
    ))
}
