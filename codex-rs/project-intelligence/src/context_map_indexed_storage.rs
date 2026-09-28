use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::ContextMapStoreError;
use crate::HierarchyNode;
use crate::IndexedExtraction;
use crate::NewContextMapEntry;
use crate::context_map_storage::coverage_name;
use crate::context_map_storage::load_entry_by_id;
use crate::context_map_storage::validate_current_source;
use crate::context_map_storage::write_routing_terms;
use crate::context_map_storage::write_search_row;
use crate::storage::load_node;
use crate::storage::unix_timestamp_millis;

const INITIAL_REVISION: i64 = 1;

pub(crate) async fn load_indexed_extraction(
    connection: &mut SqliteConnection,
    node: &HierarchyNode,
) -> Result<Option<IndexedExtraction>, ContextMapStoreError> {
    let row = sqlx::query_as::<_, StoredIndexedExtraction>(
        "SELECT extractor_name, extractor_version, canonical_representation_digest
         FROM region_extraction_attestations WHERE region_node_id = ?",
    )
    .bind(node.id.as_str())
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    IndexedExtraction::new(
        row.extractor_name,
        row.extractor_version,
        row.canonical_representation_digest,
    )
    .map(Some)
    .map_err(|_| ContextMapStoreError::CorruptEntry(node.id.to_string()))
}

#[derive(FromRow)]
struct StoredIndexedExtraction {
    extractor_name: String,
    extractor_version: String,
    canonical_representation_digest: String,
}

pub(crate) async fn upsert_indexed_entry(
    connection: &mut SqliteConnection,
    id: &crate::ContextMapEntryId,
    value: NewContextMapEntry,
) -> Result<(), ContextMapStoreError> {
    value.validate()?;
    let node = load_node(connection, &value.project_id, &value.node_id)
        .await?
        .ok_or_else(|| ContextMapStoreError::NodeNotFound(value.node_id.to_string()))?;
    validate_current_source(&value, &node)?;
    let now = unix_timestamp_millis()?;
    let Some(existing) = load_entry_by_id(connection, id).await? else {
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
        .execute(&mut *connection)
        .await?;
        write_routing_terms(connection, id, &value.routing_terms).await?;
        write_search_row(connection, insert.last_insert_rowid(), id, &value).await?;
        return Ok(());
    };
    if existing.value.project_id != value.project_id || existing.value.node_id != value.node_id {
        return Err(ContextMapStoreError::EntryIdentityConflict(id.to_string()));
    }
    if existing.value == value {
        return Ok(());
    }
    let rowid: i64 =
        sqlx::query_scalar("SELECT rowid FROM context_map_entries WHERE project_id = ? AND id = ?")
            .bind(&value.project_id)
            .bind(id.as_str())
            .fetch_one(&mut *connection)
            .await?;
    sqlx::query(
        "UPDATE context_map_entries
         SET source_fingerprint = ?, description = ?, coverage = ?,
             revision = revision + 1, updated_at_ms = ?
         WHERE project_id = ? AND id = ?",
    )
    .bind(value.source_fingerprint.as_str())
    .bind(&value.description)
    .bind(coverage_name(value.coverage))
    .bind(now)
    .bind(&value.project_id)
    .bind(id.as_str())
    .execute(&mut *connection)
    .await?;
    sqlx::query("DELETE FROM context_map_routing_terms WHERE entry_id = ?")
        .bind(id.as_str())
        .execute(&mut *connection)
        .await?;
    write_routing_terms(connection, id, &value.routing_terms).await?;
    sqlx::query("DELETE FROM context_map_search WHERE rowid = ?")
        .bind(rowid)
        .execute(&mut *connection)
        .await?;
    write_search_row(connection, rowid, id, &value).await?;
    Ok(())
}
