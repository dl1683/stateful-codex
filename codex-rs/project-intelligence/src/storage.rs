use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_state::SqliteConfig;
use sqlx::FromRow;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::HierarchyError;
use crate::HierarchyNode;
use crate::HierarchyNodeId;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;
use crate::RegionAnchor;
use crate::SourceFingerprint;

pub(crate) const DATABASE_NAME: &str = "project_intelligence_1.sqlite";
const INITIAL_REVISION: i64 = 1;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct HierarchyStore {
    pool: SqlitePool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HierarchySourceUpdate {
    pub expected_revision: u64,
    pub lifecycle: NodeLifecycle,
    pub source_fingerprint: Option<SourceFingerprint>,
}

impl HierarchyStore {
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, HierarchyStoreError> {
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

    pub async fn create_node(
        &self,
        id: HierarchyNodeId,
        value: NewHierarchyNode,
    ) -> Result<HierarchyNode, HierarchyStoreError> {
        value.validate()?;
        let now = unix_timestamp_millis()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_node_by_id(&mut transaction, &id).await? {
            if existing.value != value {
                return Err(HierarchyStoreError::NodeIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        validate_parent(&mut transaction, &value).await?;
        sqlx::query(
            "INSERT INTO hierarchy_nodes (
                id, project_id, parent_id, kind, project_root, relative_path,
                anchor_scheme, anchor_locator, source_fingerprint, lifecycle,
                revision, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                .map(SourceFingerprint::as_str),
        )
        .bind(lifecycle_name(NodeLifecycle::Active))
        .bind(INITIAL_REVISION)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let node = load_node(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| HierarchyStoreError::NodeNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(node)
    }

    pub async fn get_node(
        &self,
        project_id: &str,
        id: &HierarchyNodeId,
    ) -> Result<Option<HierarchyNode>, HierarchyStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_node(&mut connection, project_id, id).await
    }

    pub async fn project_node(
        &self,
        project_id: &str,
    ) -> Result<Option<HierarchyNode>, HierarchyStoreError> {
        let stored = sqlx::query_as::<_, StoredHierarchyNode>(
            "SELECT * FROM hierarchy_nodes
             WHERE project_id = ? AND kind = 'project'
             ORDER BY id LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        stored.map(TryInto::try_into).transpose()
    }

    pub async fn list_children(
        &self,
        project_id: &str,
        parent_id: &HierarchyNodeId,
    ) -> Result<Vec<HierarchyNode>, HierarchyStoreError> {
        let rows = sqlx::query_as::<_, StoredHierarchyNode>(
            "SELECT * FROM hierarchy_nodes
             WHERE project_id = ? AND parent_id = ?
             ORDER BY relative_path, kind, anchor_scheme, anchor_locator, id",
        )
        .bind(project_id)
        .bind(parent_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn update_source_state(
        &self,
        project_id: &str,
        id: &HierarchyNodeId,
        update: HierarchySourceUpdate,
    ) -> Result<HierarchyNode, HierarchyStoreError> {
        let expected_revision = i64::try_from(update.expected_revision)
            .map_err(|_| HierarchyStoreError::RevisionOverflow)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_node(&mut transaction, project_id, id)
            .await?
            .ok_or_else(|| HierarchyStoreError::NodeNotFound(id.to_string()))?;
        if current.revision != update.expected_revision {
            return Err(HierarchyStoreError::RevisionConflict {
                expected: update.expected_revision,
                actual: current.revision,
            });
        }
        let mut proposed_value = current.value;
        proposed_value.source_fingerprint = update.source_fingerprint;
        proposed_value.validate()?;
        let updated = sqlx::query(
            "UPDATE hierarchy_nodes
             SET source_fingerprint = ?, lifecycle = ?, revision = revision + 1,
                 updated_at_ms = ?
             WHERE project_id = ? AND id = ? AND revision = ?",
        )
        .bind(
            proposed_value
                .source_fingerprint
                .as_ref()
                .map(SourceFingerprint::as_str),
        )
        .bind(lifecycle_name(update.lifecycle))
        .bind(unix_timestamp_millis()?)
        .bind(project_id)
        .bind(id.as_str())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(HierarchyStoreError::ConcurrentMutation);
        }
        let node = load_node(&mut transaction, project_id, id)
            .await?
            .ok_or_else(|| HierarchyStoreError::NodeNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(node)
    }
}

#[derive(FromRow)]
struct StoredHierarchyNode {
    id: String,
    project_id: String,
    parent_id: Option<String>,
    kind: String,
    project_root: Option<String>,
    relative_path: String,
    anchor_scheme: Option<String>,
    anchor_locator: Option<String>,
    source_fingerprint: Option<String>,
    lifecycle: String,
    revision: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl TryFrom<StoredHierarchyNode> for HierarchyNode {
    type Error = HierarchyStoreError;

    fn try_from(stored: StoredHierarchyNode) -> Result<Self, Self::Error> {
        let region_anchor = match (stored.anchor_scheme, stored.anchor_locator) {
            (Some(scheme), Some(locator)) => Some(RegionAnchor::new(scheme, locator)?),
            (None, None) => None,
            _ => return Err(HierarchyStoreError::CorruptNode(stored.id)),
        };
        let revision = u64::try_from(stored.revision)
            .map_err(|_| HierarchyStoreError::CorruptNode(stored.id.clone()))?;
        let node = Self {
            id: HierarchyNodeId::parse(stored.id)?,
            value: NewHierarchyNode {
                project_id: stored.project_id,
                parent_id: stored.parent_id.map(HierarchyNodeId::parse).transpose()?,
                kind: parse_kind(&stored.kind)?,
                project_root: stored.project_root,
                relative_path: ProjectRelativePath::parse(stored.relative_path)?,
                region_anchor,
                source_fingerprint: stored
                    .source_fingerprint
                    .map(SourceFingerprint::parse)
                    .transpose()?,
            },
            lifecycle: parse_lifecycle(&stored.lifecycle)?,
            revision,
            created_at_ms: stored.created_at_ms,
            updated_at_ms: stored.updated_at_ms,
        };
        node.value.validate()?;
        Ok(node)
    }
}

pub(crate) async fn load_node(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &HierarchyNodeId,
) -> Result<Option<HierarchyNode>, HierarchyStoreError> {
    sqlx::query_as::<_, StoredHierarchyNode>(
        "SELECT * FROM hierarchy_nodes WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(id.as_str())
    .fetch_optional(connection)
    .await?
    .map(TryInto::try_into)
    .transpose()
}

async fn load_node_by_id(
    connection: &mut SqliteConnection,
    id: &HierarchyNodeId,
) -> Result<Option<HierarchyNode>, HierarchyStoreError> {
    sqlx::query_as::<_, StoredHierarchyNode>("SELECT * FROM hierarchy_nodes WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(connection)
        .await?
        .map(TryInto::try_into)
        .transpose()
}

async fn validate_parent(
    connection: &mut SqliteConnection,
    child: &NewHierarchyNode,
) -> Result<(), HierarchyStoreError> {
    let Some(parent_id) = &child.parent_id else {
        return Ok(());
    };
    let parent = load_node(connection, &child.project_id, parent_id)
        .await?
        .ok_or_else(|| HierarchyStoreError::ParentNotFound(parent_id.to_string()))?;
    let valid = match child.kind {
        NodeKind::Project => false,
        NodeKind::Directory if child.relative_path.is_root() => {
            parent.value.kind == NodeKind::Project
        }
        NodeKind::Directory => {
            parent.value.kind == NodeKind::Directory
                && same_root(&parent.value, child)
                && parent.value.relative_path == parent_path(&child.relative_path)?
        }
        NodeKind::File => {
            parent.value.kind == NodeKind::Directory
                && same_root(&parent.value, child)
                && parent.value.relative_path == parent_path(&child.relative_path)?
        }
        NodeKind::Region => {
            parent.value.kind == NodeKind::File
                && same_root(&parent.value, child)
                && parent.value.relative_path == child.relative_path
        }
    };
    if !valid {
        return Err(HierarchyStoreError::InvalidParent {
            parent: parent_id.to_string(),
            kind: child.kind,
        });
    }
    Ok(())
}

fn same_root(parent: &NewHierarchyNode, child: &NewHierarchyNode) -> bool {
    parent.project_root == child.project_root
}

fn parent_path(path: &ProjectRelativePath) -> Result<ProjectRelativePath, HierarchyStoreError> {
    let parent = path
        .as_str()
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    Ok(ProjectRelativePath::parse(parent)?)
}

fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Project => "project",
        NodeKind::Directory => "directory",
        NodeKind::File => "file",
        NodeKind::Region => "region",
    }
}

fn parse_kind(value: &str) -> Result<NodeKind, HierarchyStoreError> {
    match value {
        "project" => Ok(NodeKind::Project),
        "directory" => Ok(NodeKind::Directory),
        "file" => Ok(NodeKind::File),
        "region" => Ok(NodeKind::Region),
        _ => Err(HierarchyStoreError::CorruptEnum(value.to_string())),
    }
}

fn lifecycle_name(lifecycle: NodeLifecycle) -> &'static str {
    match lifecycle {
        NodeLifecycle::Active => "active",
        NodeLifecycle::Missing => "missing",
        NodeLifecycle::Replaced => "replaced",
    }
}

fn parse_lifecycle(value: &str) -> Result<NodeLifecycle, HierarchyStoreError> {
    match value {
        "active" => Ok(NodeLifecycle::Active),
        "missing" => Ok(NodeLifecycle::Missing),
        "replaced" => Ok(NodeLifecycle::Replaced),
        _ => Err(HierarchyStoreError::CorruptEnum(value.to_string())),
    }
}

pub(crate) fn unix_timestamp_millis() -> Result<i64, HierarchyStoreError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HierarchyStoreError::InvalidSystemTime)?
        .as_millis();
    i64::try_from(millis).map_err(|_| HierarchyStoreError::InvalidSystemTime)
}

#[derive(Debug, Error)]
pub enum HierarchyStoreError {
    #[error(transparent)]
    InvalidNode(#[from] HierarchyError),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("hierarchy parent not found: {0}")]
    ParentNotFound(String),
    #[error("node {kind:?} cannot be a child of hierarchy node {parent}")]
    InvalidParent { parent: String, kind: NodeKind },
    #[error("hierarchy node not found: {0}")]
    NodeNotFound(String),
    #[error("hierarchy node ID was already used for different content: {0}")]
    NodeIdentityConflict(String),
    #[error("hierarchy revision conflict: expected {expected}, found {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("hierarchy revision does not fit the storage representation")]
    RevisionOverflow,
    #[error("hierarchy node changed during a guarded mutation")]
    ConcurrentMutation,
    #[error("stored hierarchy node is corrupt: {0}")]
    CorruptNode(String),
    #[error("stored hierarchy enum value is unknown: {0}")]
    CorruptEnum(String),
    #[error("system time cannot be represented as Unix milliseconds")]
    InvalidSystemTime,
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
