//! Read the unchanged native database without migrations or write access.

use std::path::Path;
use std::time::Duration;

use codex_state::SqliteConfig;
use sqlx::SqlitePool;

use crate::BlackboardEntryId;
use crate::BlackboardHit;

use super::BlackboardStore;
use super::BlackboardStoreError;

async fn open(database: &Path) -> Result<SqlitePool, BlackboardStoreError> {
    let database = std::path::absolute(database)?;
    let home = database
        .parent()
        .ok_or_else(|| std::io::Error::other("database path has no directory"))?
        .to_path_buf();
    let sqlite = SqliteConfig::from_sqlite_home(home.try_into().map_err(std::io::Error::other)?);
    Ok(sqlite
        .open_read_only_pool(&database, Some(Duration::from_secs(/*secs*/ 5)))
        .await?)
}

/// Inspect at most 21 native project identifiers without opening an engine home.
/// Callers can use the extra row to refuse roots exceeding a 20-project page.
pub async fn native_memory_projects(
    database: &Path,
) -> Result<Vec<(String, String)>, BlackboardStoreError> {
    let pool = open(database).await?;
    let result = sqlx::query_as(
        "SELECT project_id, id FROM hierarchy_nodes WHERE kind = 'project'
         AND octet_length(project_id) <= 512 AND octet_length(id) <= 512
         ORDER BY project_id, id LIMIT 21",
    )
    .fetch_all(&pool)
    .await;
    pool.close().await;
    Ok(result?)
}

/// Read one native entry under the same eligibility and retirement rules used
/// by model tools. The existing database is opened read-only and never migrated.
/// A miss includes unavailable identity coverage and retired/ineligible entries.
pub async fn read_native_memory(
    database: &Path,
    project_id: &str,
    entry_id: &BlackboardEntryId,
) -> Result<Option<BlackboardHit>, BlackboardStoreError> {
    let store = BlackboardStore {
        pool: open(database).await?,
    };
    let result = store.get_hit(project_id, entry_id).await;
    store.pool.close().await;
    result
}
