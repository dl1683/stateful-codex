use codex_state::SqliteConfig;
use sqlx::FromRow;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::ContextMapCoverage;
use crate::ContextMapEntry;
use crate::ContextMapEntryId;
use crate::ContextMapError;
use crate::ContextMapFreshness;
use crate::ContextMapHit;
use crate::ContextMapQuery;
use crate::HierarchyNode;
use crate::HierarchyNodeId;
use crate::NewContextMapEntry;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::SourceFingerprint;
use crate::storage::DATABASE_NAME;
use crate::storage::HierarchyStoreError;
use crate::storage::load_node;
use crate::storage::unix_timestamp_millis;

const INITIAL_REVISION: i64 = 1;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct ContextMapStore {
    pool: SqlitePool,
}

impl ContextMapStore {
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, ContextMapStoreError> {
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
            .await?;
        if let Err(error) = MIGRATOR.run(&pool).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool })
    }

    pub async fn create_entry(
        &self,
        id: ContextMapEntryId,
        value: NewContextMapEntry,
    ) -> Result<ContextMapEntry, ContextMapStoreError> {
        value.validate()?;
        let now = unix_timestamp_millis()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let node = load_node(&mut transaction, &value.project_id, &value.node_id)
            .await?
            .ok_or_else(|| ContextMapStoreError::NodeNotFound(value.node_id.to_string()))?;
        validate_current_source(&value, &node)?;
        let insert = sqlx::query(
            "INSERT INTO context_map_entries (
                id, project_id, node_id, source_fingerprint, description, coverage,
                revision, created_at_ms, updated_at_ms, last_verified_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind(id.as_str())
        .bind(&value.project_id)
        .bind(value.node_id.as_str())
        .bind(value.source_fingerprint.as_str())
        .bind(&value.description)
        .bind(coverage_name(value.coverage))
        .bind(INITIAL_REVISION)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        for (position, term) in value.routing_terms.iter().enumerate() {
            let position = i64::try_from(position)
                .map_err(|_| ContextMapStoreError::RoutingTermPositionOverflow)?;
            sqlx::query(
                "INSERT INTO context_map_routing_terms (entry_id, position, term)
                 VALUES (?, ?, ?)",
            )
            .bind(id.as_str())
            .bind(position)
            .bind(term)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT INTO context_map_search (
                rowid, entry_id, project_id, description, routing_terms
             ) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(insert.last_insert_rowid())
        .bind(id.as_str())
        .bind(&value.project_id)
        .bind(&value.description)
        .bind(value.routing_terms.join(" "))
        .execute(&mut *transaction)
        .await?;
        let entry = load_entry(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(entry)
    }

    pub async fn get_entry(
        &self,
        project_id: &str,
        id: &ContextMapEntryId,
    ) -> Result<Option<ContextMapEntry>, ContextMapStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_entry(&mut connection, project_id, id).await
    }

    pub async fn query(
        &self,
        query: ContextMapQuery,
    ) -> Result<Vec<ContextMapHit>, ContextMapStoreError> {
        query.validate()?;
        let expression = search_expression(&query.text)?;
        let limit = i64::from(query.max_results);
        let mut connection = self.pool.acquire().await?;
        let entry_ids = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM context_map_search AS search
             JOIN context_map_entries AS entry ON entry.rowid = search.rowid
             WHERE context_map_search MATCH ? AND entry.project_id = ?
             ORDER BY bm25(context_map_search), entry.id
             LIMIT ?",
        )
        .bind(expression)
        .bind(&query.project_id)
        .bind(limit)
        .fetch_all(&mut *connection)
        .await?;
        let mut hits = Vec::with_capacity(entry_ids.len());
        for raw_id in entry_ids {
            let id = ContextMapEntryId::parse(&raw_id)
                .map_err(|_| ContextMapStoreError::CorruptEntry(raw_id))?;
            let entry = load_entry(&mut connection, &query.project_id, &id)
                .await?
                .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?;
            let node = load_node(&mut connection, &query.project_id, &entry.value.node_id)
                .await?
                .ok_or_else(|| {
                    ContextMapStoreError::NodeNotFound(entry.value.node_id.to_string())
                })?;
            let freshness = entry.freshness_against(&node)?;
            hits.push(ContextMapHit { entry, freshness });
        }
        Ok(hits)
    }
}

#[derive(FromRow)]
struct StoredContextMapEntry {
    id: String,
    project_id: String,
    node_id: String,
    source_fingerprint: String,
    description: String,
    coverage: String,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    last_verified_at_ms: Option<i64>,
}

async fn load_entry(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &ContextMapEntryId,
) -> Result<Option<ContextMapEntry>, ContextMapStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredContextMapEntry>(
        "SELECT * FROM context_map_entries WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let routing_terms = sqlx::query_scalar::<_, String>(
        "SELECT term FROM context_map_routing_terms
         WHERE entry_id = ? ORDER BY position",
    )
    .bind(&stored.id)
    .fetch_all(connection)
    .await?;
    let revision = u64::try_from(stored.revision)
        .map_err(|_| ContextMapStoreError::CorruptEntry(stored.id.clone()))?;
    let node_id = HierarchyNodeId::parse(stored.node_id)
        .map_err(|_| ContextMapStoreError::CorruptEntry(stored.id.clone()))?;
    let source_fingerprint = SourceFingerprint::parse(stored.source_fingerprint)
        .map_err(|_| ContextMapStoreError::CorruptEntry(stored.id.clone()))?;
    let entry_id = ContextMapEntryId::parse(&stored.id)
        .map_err(|_| ContextMapStoreError::CorruptEntry(stored.id.clone()))?;
    let entry = ContextMapEntry {
        id: entry_id,
        value: NewContextMapEntry {
            project_id: stored.project_id,
            node_id,
            source_fingerprint,
            description: stored.description,
            routing_terms,
            coverage: parse_coverage(&stored.coverage)?,
        },
        revision,
        created_at_ms: stored.created_at_ms,
        updated_at_ms: stored.updated_at_ms,
        last_verified_at_ms: stored.last_verified_at_ms,
    };
    entry
        .value
        .validate()
        .map_err(|_| ContextMapStoreError::CorruptEntry(entry.id.to_string()))?;
    Ok(Some(entry))
}

fn validate_current_source(
    value: &NewContextMapEntry,
    node: &HierarchyNode,
) -> Result<(), ContextMapStoreError> {
    if value.project_id != node.value.project_id || value.node_id != node.id {
        return Err(ContextMapError::HierarchyBindingMismatch.into());
    }
    if !matches!(node.value.kind, NodeKind::File | NodeKind::Region) {
        return Err(ContextMapError::UnsupportedNodeKind(node.value.kind).into());
    }
    let freshness = match node.lifecycle {
        NodeLifecycle::Missing => ContextMapFreshness::SourceUnavailable,
        NodeLifecycle::Replaced => ContextMapFreshness::Stale,
        NodeLifecycle::Active => {
            if node.value.source_fingerprint.as_ref() == Some(&value.source_fingerprint) {
                return Ok(());
            }
            ContextMapFreshness::Stale
        }
    };
    Err(ContextMapStoreError::SourceNotCurrent(freshness))
}

fn coverage_name(coverage: ContextMapCoverage) -> &'static str {
    match coverage {
        ContextMapCoverage::Complete => "complete",
        ContextMapCoverage::Partial => "partial",
    }
}

fn parse_coverage(value: &str) -> Result<ContextMapCoverage, ContextMapStoreError> {
    match value {
        "complete" => Ok(ContextMapCoverage::Complete),
        "partial" => Ok(ContextMapCoverage::Partial),
        _ => Err(ContextMapStoreError::CorruptEnum(value.to_string())),
    }
}

fn search_expression(text: &str) -> Result<String, ContextMapStoreError> {
    let terms = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .take(32)
        .map(|term| format!("\"{term}\"*"))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return Err(ContextMapError::NoSearchTerms.into());
    }
    Ok(terms.join(" OR "))
}

#[derive(Debug, Error)]
pub enum ContextMapStoreError {
    #[error(transparent)]
    InvalidEntry(#[from] ContextMapError),
    #[error(transparent)]
    Hierarchy(#[from] HierarchyStoreError),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("hierarchy node not found for context map: {0}")]
    NodeNotFound(String),
    #[error("context-map source is not current: {0:?}")]
    SourceNotCurrent(ContextMapFreshness),
    #[error("context-map entry not found: {0}")]
    EntryNotFound(String),
    #[error("context-map routing-term position overflow")]
    RoutingTermPositionOverflow,
    #[error("stored context-map entry is corrupt: {0}")]
    CorruptEntry(String),
    #[error("stored context-map enum value is unknown: {0}")]
    CorruptEnum(String),
}

#[cfg(test)]
#[path = "context_map_storage_tests.rs"]
mod tests;
