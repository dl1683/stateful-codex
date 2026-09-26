use std::collections::HashSet;

use sqlx::SqliteConnection;

use crate::ContextMapEntryId;
use crate::HierarchyNodeId;
use crate::NewContextMapEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;
use crate::RegionAnchor;
use crate::context_map_storage::upsert_indexed_entry;
use crate::storage::kind_name;
use crate::storage::lifecycle_name;
use crate::storage::load_node;
use crate::storage::unix_timestamp_millis;

use super::ProjectIndexer;
use super::ProjectIndexerError;
use super::scan::ScannedFile;
use super::stable_id;
use super::stable_id_text;

const INITIAL_REVISION: i64 = 1;

pub(super) async fn publish_file(
    indexer: &ProjectIndexer,
    project_id: &str,
    file_id: &HierarchyNodeId,
    parent_id: HierarchyNodeId,
    file: &ScannedFile,
) -> Result<(), ProjectIndexerError> {
    let relative_path = ProjectRelativePath::parse(&file.relative_path)?;
    let file_node = NewHierarchyNode {
        project_id: project_id.to_string(),
        parent_id: Some(parent_id),
        kind: NodeKind::File,
        project_root: Some(file.project_root.clone()),
        relative_path: relative_path.clone(),
        region_anchor: None,
        source_fingerprint: Some(file.fingerprint.clone()),
    };
    let file_context_id = ContextMapEntryId::parse(stable_id_text(
        "context",
        &[project_id, &file.project_root, &file.relative_path],
    ))?;
    let file_context = NewContextMapEntry {
        project_id: project_id.to_string(),
        node_id: file_id.clone(),
        source_fingerprint: file.fingerprint.clone(),
        description: file.description.clone(),
        routing_terms: file.routing_terms.clone(),
        coverage: file.coverage,
    };
    let mut transaction = indexer.context_map.begin_immediate().await?;
    upsert_indexed_node(&mut transaction, file_id, file_node).await?;
    upsert_indexed_entry(&mut transaction, &file_context_id, file_context).await?;

    let mut active_region_ids = HashSet::with_capacity(file.regions.len());
    for (cell_index, region) in file.regions.iter().enumerate() {
        let cell = cell_index.to_string();
        let locator = format!("{}-{}", region.start_line, region.end_line);
        let region_id = stable_id(
            "region",
            &[project_id, &file.project_root, &file.relative_path, &cell],
        )?;
        let region_node = NewHierarchyNode {
            project_id: project_id.to_string(),
            parent_id: Some(file_id.clone()),
            kind: NodeKind::Region,
            project_root: Some(file.project_root.clone()),
            relative_path: relative_path.clone(),
            region_anchor: Some(RegionAnchor::new("lines", locator)?),
            source_fingerprint: Some(file.fingerprint.clone()),
        };
        upsert_indexed_node(&mut transaction, &region_id, region_node).await?;
        let context_id = ContextMapEntryId::parse(stable_id_text(
            "context",
            &[
                project_id,
                &file.project_root,
                &file.relative_path,
                "cell",
                &cell,
            ],
        ))?;
        upsert_indexed_entry(
            &mut transaction,
            &context_id,
            NewContextMapEntry {
                project_id: project_id.to_string(),
                node_id: region_id.clone(),
                source_fingerprint: file.fingerprint.clone(),
                description: region.description.clone(),
                routing_terms: Vec::new(),
                coverage: region.coverage,
            },
        )
        .await?;
        active_region_ids.insert(region_id);
    }
    retire_absent_regions(
        &mut transaction,
        project_id,
        file_id,
        &active_region_ids,
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub(super) async fn mark_file_missing(
    indexer: &ProjectIndexer,
    project_id: &str,
    file_id: &HierarchyNodeId,
) -> Result<(), ProjectIndexerError> {
    let mut transaction = indexer.context_map.begin_immediate().await?;
    let file = load_node(&mut transaction, project_id, file_id)
        .await?
        .ok_or_else(|| ProjectIndexerError::SourceNotIndexed(file_id.to_string()))?;
    if file.value.kind != NodeKind::File {
        return Err(ProjectIndexerError::IdentityConflict(file_id.to_string()));
    }
    let now = unix_timestamp_millis()?;
    sqlx::query(
        "UPDATE hierarchy_nodes
         SET lifecycle = 'missing', revision = revision + 1, updated_at_ms = ?
         WHERE project_id = ? AND parent_id = ? AND kind = 'region'
           AND lifecycle = 'active'",
    )
    .bind(now)
    .bind(project_id)
    .bind(file_id.as_str())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE hierarchy_nodes
         SET lifecycle = 'missing', revision = revision + 1, updated_at_ms = ?
         WHERE project_id = ? AND id = ? AND lifecycle = 'active'",
    )
    .bind(now)
    .bind(project_id)
    .bind(file_id.as_str())
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn upsert_indexed_node(
    connection: &mut SqliteConnection,
    id: &HierarchyNodeId,
    value: NewHierarchyNode,
) -> Result<(), ProjectIndexerError> {
    value.validate()?;
    let Some(existing) = load_node(connection, &value.project_id, id).await? else {
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO hierarchy_nodes (
                id, project_id, parent_id, kind, project_root, relative_path,
                anchor_scheme, anchor_locator, source_fingerprint, lifecycle,
                revision, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(&value.project_id)
        .bind(value.parent_id.as_ref().map(HierarchyNodeId::as_str))
        .bind(kind_name(value.kind))
        .bind(&value.project_root)
        .bind(value.relative_path.as_str())
        .bind(value.region_anchor.as_ref().map(|anchor| &anchor.scheme))
        .bind(value.region_anchor.as_ref().map(|anchor| &anchor.locator))
        .bind(
            value
                .source_fingerprint
                .as_ref()
                .map(crate::SourceFingerprint::as_str),
        )
        .bind(INITIAL_REVISION)
        .bind(now)
        .bind(now)
        .execute(connection)
        .await?;
        return Ok(());
    };
    let mut identity = value.clone();
    identity.region_anchor = existing.value.region_anchor.clone();
    identity.source_fingerprint = existing.value.source_fingerprint.clone();
    if existing.value != identity {
        return Err(ProjectIndexerError::IdentityConflict(id.to_string()));
    }
    if existing.lifecycle == NodeLifecycle::Active
        && existing.value.region_anchor == value.region_anchor
        && existing.value.source_fingerprint == value.source_fingerprint
    {
        return Ok(());
    }
    let now = unix_timestamp_millis()?;
    sqlx::query(
        "UPDATE hierarchy_nodes
         SET anchor_scheme = ?, anchor_locator = ?, source_fingerprint = ?,
             lifecycle = ?, revision = revision + 1, updated_at_ms = ?
         WHERE project_id = ? AND id = ?",
    )
    .bind(value.region_anchor.as_ref().map(|anchor| &anchor.scheme))
    .bind(value.region_anchor.as_ref().map(|anchor| &anchor.locator))
    .bind(
        value
            .source_fingerprint
            .as_ref()
            .map(crate::SourceFingerprint::as_str),
    )
    .bind(lifecycle_name(NodeLifecycle::Active))
    .bind(now)
    .bind(&value.project_id)
    .bind(id.as_str())
    .execute(connection)
    .await?;
    Ok(())
}

async fn retire_absent_regions(
    connection: &mut SqliteConnection,
    project_id: &str,
    file_id: &HierarchyNodeId,
    active_region_ids: &HashSet<HierarchyNodeId>,
) -> Result<(), ProjectIndexerError> {
    let stored_ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM hierarchy_nodes
         WHERE project_id = ? AND parent_id = ? AND kind = 'region'
           AND lifecycle = 'active'",
    )
    .bind(project_id)
    .bind(file_id.as_str())
    .fetch_all(&mut *connection)
    .await?;
    let now = unix_timestamp_millis()?;
    for raw_id in stored_ids {
        let id = HierarchyNodeId::parse(raw_id)?;
        if active_region_ids.contains(&id) {
            continue;
        }
        sqlx::query(
            "UPDATE hierarchy_nodes
             SET lifecycle = 'missing', revision = revision + 1, updated_at_ms = ?
             WHERE project_id = ? AND id = ? AND lifecycle = 'active'",
        )
        .bind(now)
        .bind(project_id)
        .bind(id.as_str())
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}
