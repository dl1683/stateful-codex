use std::collections::HashMap;
use std::collections::HashSet;

use codex_state::SqliteConfig;
use sqlx::FromRow;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::ContextMapCoverage;
use crate::ContextMapEntry;
use crate::ContextMapEntryId;
use crate::ContextMapEntryUpdate;
use crate::ContextMapError;
use crate::ContextMapFreshness;
use crate::ContextMapHit;
use crate::ContextMapListQuery;
use crate::ContextMapQuery;
use crate::HierarchyNode;
use crate::HierarchyNodeId;
use crate::NewContextMapEntry;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;
use crate::SourceFingerprint;
use crate::search::literal_expression;
use crate::search::literal_prefix_expression;
use crate::storage::DATABASE_NAME;
use crate::storage::HierarchyStoreError;
use crate::storage::load_node;
use crate::storage::unix_timestamp_millis;

const INITIAL_REVISION: i64 = 1;
const MAX_QUERY_HITS_PER_SOURCE: usize = 3;
const MAX_QUERY_HITS_PER_DIRECTORY: usize = 3;
const CANDIDATE_OVERFETCH_FACTOR: u32 = 16;
const MAX_CANDIDATE_PAGES: i64 = 16;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(FromRow)]
struct SearchCandidate {
    id: String,
    project_root: String,
    relative_path: String,
    kind: String,
    lifecycle: String,
    node_source_fingerprint: Option<String>,
    entry_source_fingerprint: String,
    parent_kind: Option<String>,
    parent_lifecycle: Option<String>,
    parent_source_fingerprint: Option<String>,
    score: f64,
}

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

    pub(crate) async fn begin_immediate(
        &self,
    ) -> Result<sqlx::Transaction<'_, sqlx::Sqlite>, sqlx::Error> {
        self.pool.begin_with("BEGIN IMMEDIATE").await
    }

    pub async fn create_entry(
        &self,
        id: ContextMapEntryId,
        value: NewContextMapEntry,
    ) -> Result<ContextMapEntry, ContextMapStoreError> {
        value.validate()?;
        let now = unix_timestamp_millis()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_entry_by_id(&mut transaction, &id).await? {
            if existing.value != value {
                return Err(ContextMapStoreError::EntryIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
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
        write_routing_terms(&mut transaction, &id, &value.routing_terms).await?;
        write_search_row(&mut transaction, insert.last_insert_rowid(), &id, &value).await?;
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

    pub async fn get_hit(
        &self,
        project_id: &str,
        id: &ContextMapEntryId,
    ) -> Result<Option<ContextMapHit>, ContextMapStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_hit(&mut connection, project_id, id).await
    }

    pub async fn get_guarded_hit(
        &self,
        project_id: &str,
        id: &ContextMapEntryId,
        expected_fingerprint: &SourceFingerprint,
    ) -> Result<Option<ContextMapHit>, ContextMapStoreError> {
        let mut connection = self.pool.acquire().await?;
        let Some(hit) = load_hit(&mut connection, project_id, id).await? else {
            return Ok(None);
        };
        if &hit.entry.value.source_fingerprint != expected_fingerprint {
            return Err(ContextMapStoreError::SourceNotCurrent(
                ContextMapFreshness::Stale,
            ));
        }
        if hit.source.region_anchor.is_some() {
            let current: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM context_map_entries AS entry
                 JOIN hierarchy_nodes AS node ON node.id = entry.node_id
                 JOIN hierarchy_nodes AS parent
                   ON parent.id = node.parent_id AND parent.project_id = node.project_id
                 WHERE entry.project_id = ? AND entry.id = ?
                   AND node.kind = 'region' AND node.lifecycle = 'active'
                   AND node.source_fingerprint = entry.source_fingerprint
                   AND parent.kind = 'file' AND parent.lifecycle = 'active'
                   AND parent.source_fingerprint = entry.source_fingerprint",
            )
            .bind(project_id)
            .bind(id.as_str())
            .fetch_one(&mut *connection)
            .await?;
            if current != 1 {
                return Err(ContextMapStoreError::SourceNotCurrent(
                    ContextMapFreshness::Stale,
                ));
            }
        }
        Ok(Some(hit))
    }

    pub async fn file_hits_for_path(
        &self,
        project_id: &str,
        relative_path: &ProjectRelativePath,
    ) -> Result<Vec<ContextMapHit>, ContextMapStoreError> {
        let mut connection = self.pool.acquire().await?;
        let raw_ids = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM context_map_entries AS entry
             JOIN hierarchy_nodes AS node ON node.id = entry.node_id
             WHERE entry.project_id = ? AND node.project_id = ?
               AND node.kind = 'file' AND node.relative_path = ?
             ORDER BY node.project_root, entry.id",
        )
        .bind(project_id)
        .bind(project_id)
        .bind(relative_path.as_str())
        .fetch_all(&mut *connection)
        .await?;
        let mut hits = Vec::with_capacity(raw_ids.len());
        for raw_id in raw_ids {
            let id = ContextMapEntryId::parse(&raw_id)
                .map_err(|_| ContextMapStoreError::CorruptEntry(raw_id))?;
            hits.push(
                load_hit(&mut connection, project_id, &id)
                    .await?
                    .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?,
            );
        }
        Ok(hits)
    }

    pub async fn query(
        &self,
        query: ContextMapQuery,
    ) -> Result<Vec<ContextMapHit>, ContextMapStoreError> {
        query.validate()?;
        let expression = search_expression(&query.text)?;
        let candidate_target =
            i64::from(query.max_results.saturating_mul(CANDIDATE_OVERFETCH_FACTOR));
        let candidate_scan_limit = candidate_target.saturating_mul(MAX_CANDIDATE_PAGES);
        let candidate_target =
            usize::try_from(candidate_target).map_err(|_| ContextMapError::InvalidQuery)?;
        let mut connection = self.pool.acquire().await?;
        let mut candidates = Vec::new();
        let mut offset = 0_i64;
        while candidates.len() < candidate_target && offset < candidate_scan_limit {
            let page_limit = i64::try_from(candidate_target)
                .unwrap_or(i64::MAX)
                .min(candidate_scan_limit - offset);
            let page = sqlx::query_as::<_, SearchCandidate>(
                "SELECT entry.id, node.project_root, node.relative_path, node.kind,
                        node.lifecycle, node.source_fingerprint AS node_source_fingerprint,
                        entry.source_fingerprint AS entry_source_fingerprint,
                        parent.kind AS parent_kind, parent.lifecycle AS parent_lifecycle,
                        parent.source_fingerprint AS parent_source_fingerprint,
                        ranked.score
                 FROM (
                   SELECT rowid, rank AS score
                   FROM context_map_search
                   WHERE context_map_search MATCH ? AND project_id = ?
                   ORDER BY rank, rowid
                   LIMIT ? OFFSET ?
                 ) AS ranked
                 JOIN context_map_entries AS entry ON entry.rowid = ranked.rowid
                 JOIN hierarchy_nodes AS node ON node.id = entry.node_id
                 LEFT JOIN hierarchy_nodes AS parent
                   ON parent.id = node.parent_id AND parent.project_id = node.project_id
                 WHERE entry.project_id = ?
                 ORDER BY ranked.score, entry.id",
            )
            .bind(&expression)
            .bind(&query.project_id)
            .bind(page_limit)
            .bind(offset)
            .bind(&query.project_id)
            .fetch_all(&mut *connection)
            .await?;
            let page_len = i64::try_from(page.len()).unwrap_or(i64::MAX);
            offset = offset.saturating_add(page_len);
            let remaining = candidate_target.saturating_sub(candidates.len());
            candidates.extend(
                page.into_iter()
                    .filter(SearchCandidate::is_queryable)
                    .take(remaining),
            );
            if page_len < page_limit {
                break;
            }
        }
        candidates.sort_by(|left, right| {
            left.score
                .total_cmp(&right.score)
                .then_with(|| left.id.cmp(&right.id))
        });
        let sources_with_regions = candidates
            .iter()
            .filter(|candidate| candidate.kind == "region")
            .map(SearchCandidate::source_key)
            .collect::<HashSet<_>>();
        let max_results =
            usize::try_from(query.max_results).map_err(|_| ContextMapError::InvalidQuery)?;
        let mut source_hits = HashMap::new();
        let mut directory_hits = HashMap::new();
        let mut entry_ids = Vec::with_capacity(max_results);
        for candidate in candidates {
            if !matches!(candidate.kind.as_str(), "file" | "region") {
                return Err(ContextMapStoreError::CorruptEntry(candidate.id));
            }
            let source_key = candidate.source_key();
            if candidate.kind == "file" && sources_with_regions.contains(&source_key) {
                continue;
            }
            let hits = source_hits.entry(source_key).or_insert(0_usize);
            if *hits == MAX_QUERY_HITS_PER_SOURCE {
                continue;
            }
            let directory_hits = directory_hits
                .entry(candidate.directory_key())
                .or_insert(0_usize);
            if *directory_hits == MAX_QUERY_HITS_PER_DIRECTORY {
                continue;
            }
            *hits += 1;
            *directory_hits += 1;
            entry_ids.push(candidate.id);
            if entry_ids.len() == max_results {
                break;
            }
        }
        let mut hits = Vec::with_capacity(entry_ids.len());
        for raw_id in entry_ids {
            let id = ContextMapEntryId::parse(&raw_id)
                .map_err(|_| ContextMapStoreError::CorruptEntry(raw_id))?;
            hits.push(
                load_hit(&mut connection, &query.project_id, &id)
                    .await?
                    .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?,
            );
        }
        Ok(hits)
    }

    pub async fn list_project(
        &self,
        query: ContextMapListQuery,
    ) -> Result<Vec<ContextMapHit>, ContextMapStoreError> {
        query.validate()?;
        let limit = i64::from(query.max_results);
        let mut connection = self.pool.acquire().await?;
        let entry_ids = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM context_map_entries AS entry
             JOIN hierarchy_nodes AS node ON node.id = entry.node_id
             WHERE entry.project_id = ? AND node.project_id = ?
               AND node.kind = 'file' AND node.lifecycle = 'active'
             ORDER BY node.project_root, node.relative_path, entry.id
             LIMIT ?",
        )
        .bind(&query.project_id)
        .bind(&query.project_id)
        .bind(limit)
        .fetch_all(&mut *connection)
        .await?;
        let mut hits = Vec::with_capacity(entry_ids.len());
        for raw_id in entry_ids {
            let id = ContextMapEntryId::parse(&raw_id)
                .map_err(|_| ContextMapStoreError::CorruptEntry(raw_id))?;
            hits.push(
                load_hit(&mut connection, &query.project_id, &id)
                    .await?
                    .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?,
            );
        }
        Ok(hits)
    }

    pub async fn update_entry(
        &self,
        project_id: &str,
        id: &ContextMapEntryId,
        update: ContextMapEntryUpdate,
    ) -> Result<ContextMapEntry, ContextMapStoreError> {
        let expected_revision = i64::try_from(update.expected_revision)
            .map_err(|_| ContextMapStoreError::RevisionOverflow)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_entry(&mut transaction, project_id, id)
            .await?
            .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?;
        if current.revision != update.expected_revision {
            return Err(ContextMapStoreError::RevisionConflict {
                expected: update.expected_revision,
                actual: current.revision,
            });
        }
        let value = NewContextMapEntry {
            project_id: current.value.project_id,
            node_id: current.value.node_id,
            source_fingerprint: update.source_fingerprint,
            description: update.description,
            routing_terms: update.routing_terms,
            coverage: update.coverage,
        };
        value.validate()?;
        let node = load_node(&mut transaction, project_id, &value.node_id)
            .await?
            .ok_or_else(|| ContextMapStoreError::NodeNotFound(value.node_id.to_string()))?;
        validate_current_source(&value, &node)?;
        let rowid: i64 = sqlx::query_scalar(
            "SELECT rowid FROM context_map_entries WHERE project_id = ? AND id = ?",
        )
        .bind(project_id)
        .bind(id.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let updated = sqlx::query(
            "UPDATE context_map_entries
             SET source_fingerprint = ?, description = ?, coverage = ?,
                 revision = revision + 1, updated_at_ms = ?
             WHERE project_id = ? AND id = ? AND revision = ?",
        )
        .bind(value.source_fingerprint.as_str())
        .bind(&value.description)
        .bind(coverage_name(value.coverage))
        .bind(unix_timestamp_millis()?)
        .bind(project_id)
        .bind(id.as_str())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(ContextMapStoreError::ConcurrentMutation);
        }
        sqlx::query("DELETE FROM context_map_routing_terms WHERE entry_id = ?")
            .bind(id.as_str())
            .execute(&mut *transaction)
            .await?;
        write_routing_terms(&mut transaction, id, &value.routing_terms).await?;
        sqlx::query("DELETE FROM context_map_search WHERE rowid = ?")
            .bind(rowid)
            .execute(&mut *transaction)
            .await?;
        write_search_row(&mut transaction, rowid, id, &value).await?;
        let entry = load_entry(&mut transaction, project_id, id)
            .await?
            .ok_or_else(|| ContextMapStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(entry)
    }
}

impl SearchCandidate {
    fn is_queryable(&self) -> bool {
        match self.kind.as_str() {
            "file" => true,
            "region" => {
                self.lifecycle == "active"
                    && self.node_source_fingerprint.as_deref()
                        == Some(self.entry_source_fingerprint.as_str())
                    && self.parent_kind.as_deref() == Some("file")
                    && self.parent_lifecycle.as_deref() == Some("active")
                    && self.parent_source_fingerprint.as_deref()
                        == Some(self.entry_source_fingerprint.as_str())
            }
            _ => true,
        }
    }

    fn source_key(&self) -> (String, String) {
        (self.project_root.clone(), self.relative_path.clone())
    }

    fn directory_key(&self) -> (String, String) {
        let directory = self
            .relative_path
            .split_once('/')
            .map_or(self.relative_path.as_str(), |(directory, _)| directory);
        (self.project_root.clone(), directory.to_string())
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

async fn load_hit(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &ContextMapEntryId,
) -> Result<Option<ContextMapHit>, ContextMapStoreError> {
    let Some(entry) = load_entry(connection, project_id, id).await? else {
        return Ok(None);
    };
    let node = load_node(connection, project_id, &entry.value.node_id)
        .await?
        .ok_or_else(|| ContextMapStoreError::NodeNotFound(entry.value.node_id.to_string()))?;
    let freshness = entry.freshness_against(&node)?;
    let source = entry.source_route(&node)?;
    Ok(Some(ContextMapHit {
        entry,
        source,
        freshness,
    }))
}

pub(crate) async fn upsert_indexed_entry(
    connection: &mut SqliteConnection,
    id: &ContextMapEntryId,
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

async fn load_entry_by_id(
    connection: &mut SqliteConnection,
    id: &ContextMapEntryId,
) -> Result<Option<ContextMapEntry>, ContextMapStoreError> {
    let project_id: Option<String> =
        sqlx::query_scalar("SELECT project_id FROM context_map_entries WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(&mut *connection)
            .await?;
    match project_id {
        Some(project_id) => load_entry(connection, &project_id, id).await,
        None => Ok(None),
    }
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
    let term_count = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| !term.is_empty())
        .take(2)
        .count();
    let expression = if term_count == 1 {
        literal_prefix_expression(text)
    } else {
        literal_expression(text)
    };
    expression.ok_or_else(|| ContextMapError::NoSearchTerms.into())
}

async fn write_routing_terms(
    connection: &mut SqliteConnection,
    entry_id: &ContextMapEntryId,
    routing_terms: &[String],
) -> Result<(), ContextMapStoreError> {
    for (position, term) in routing_terms.iter().enumerate() {
        let position = i64::try_from(position)
            .map_err(|_| ContextMapStoreError::RoutingTermPositionOverflow)?;
        sqlx::query(
            "INSERT INTO context_map_routing_terms (entry_id, position, term)
             VALUES (?, ?, ?)",
        )
        .bind(entry_id.as_str())
        .bind(position)
        .bind(term)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn write_search_row(
    connection: &mut SqliteConnection,
    rowid: i64,
    entry_id: &ContextMapEntryId,
    value: &NewContextMapEntry,
) -> Result<(), ContextMapStoreError> {
    sqlx::query(
        "INSERT INTO context_map_search (
            rowid, entry_id, project_id, description, routing_terms
         ) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(rowid)
    .bind(entry_id.as_str())
    .bind(&value.project_id)
    .bind(&value.description)
    .bind(value.routing_terms.join(" "))
    .execute(connection)
    .await?;
    Ok(())
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
    #[error("context-map entry ID was already used for different content: {0}")]
    EntryIdentityConflict(String),
    #[error("context-map routing-term position overflow")]
    RoutingTermPositionOverflow,
    #[error("context-map revision conflict: expected {expected}, found {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("context-map revision does not fit the storage representation")]
    RevisionOverflow,
    #[error("context-map entry changed during a guarded mutation")]
    ConcurrentMutation,
    #[error("stored context-map entry is corrupt: {0}")]
    CorruptEntry(String),
    #[error("stored context-map enum value is unknown: {0}")]
    CorruptEnum(String),
}

#[cfg(test)]
#[path = "context_map_storage_tests.rs"]
mod tests;
