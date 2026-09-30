use codex_state::SqliteConfig;
use sqlx::FromRow;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use thiserror::Error;

use crate::RepositoryDirtyCoverage;
use crate::RepositoryHead;
use crate::RepositoryObservation;
use crate::RepositoryObservationError;
use crate::RepositoryObservationId;
use crate::RepositoryRootObservation;
use crate::RepositoryRootsCoverage;
use crate::RepositoryUnknownReason;
use crate::RepositoryWorktree;
use crate::storage::DATABASE_NAME;

const FORMAT_VERSION: i64 = 1;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(FromRow)]
struct StoredObservation {
    project_id: String,
    format_version: i64,
    started_at_ms: i64,
    completed_at_ms: i64,
    roots_digest: String,
    roots_coverage: String,
    omitted_root_count: i64,
}

#[derive(FromRow)]
struct StoredRoot {
    project_root: String,
    git_worktree_root: Option<String>,
    head_state: String,
    head_oid: Option<String>,
    head_ref: Option<String>,
    worktree_state: String,
    dirty_digest: Option<String>,
    dirty_coverage: String,
    unknown_reason: Option<String>,
}

/// Append-only store of immutable repository observations.
///
/// Observations are sampling events, not intelligence changes, so recording
/// one never advances `project_intelligence_revisions`.
#[derive(Clone)]
pub struct RepositoryObservationStore {
    pool: SqlitePool,
}

impl RepositoryObservationStore {
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, RepositoryObservationStoreError> {
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join(DATABASE_NAME))
            .await?;
        if let Err(error) = sqlite.run_migrations(&pool, &MIGRATOR).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool })
    }

    /// Records an observation atomically. Retrying identical data succeeds;
    /// different data under an existing ID is an identity conflict.
    pub async fn record(
        &self,
        mut observation: RepositoryObservation,
    ) -> Result<(), RepositoryObservationStoreError> {
        observation.validate()?;
        observation
            .roots
            .sort_by(|left, right| left.project_root.cmp(&right.project_root));
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_observation(&mut transaction, &observation.id).await? {
            if existing != observation {
                return Err(RepositoryObservationStoreError::IdentityConflict(
                    observation.id.to_string(),
                ));
            }
            transaction.commit().await?;
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO repository_observations (
                id, project_id, format_version, started_at_ms, completed_at_ms,
                roots_digest, roots_coverage, omitted_root_count
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(observation.id.as_str())
        .bind(&observation.project_id)
        .bind(FORMAT_VERSION)
        .bind(observation.started_at_ms)
        .bind(observation.completed_at_ms)
        .bind(&observation.roots_digest)
        .bind(match observation.roots_coverage {
            RepositoryRootsCoverage::Complete => "complete",
            RepositoryRootsCoverage::Unknown => "unknown",
        })
        .bind(i64::from(observation.omitted_root_count))
        .execute(&mut *transaction)
        .await?;
        for root in &observation.roots {
            insert_root(&mut transaction, &observation.id, root).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Returns the observation only when it belongs to `project_id`.
    pub async fn get(
        &self,
        project_id: &str,
        id: &RepositoryObservationId,
    ) -> Result<Option<RepositoryObservation>, RepositoryObservationStoreError> {
        let mut transaction = self.pool.begin().await?;
        let observation = load_observation(&mut transaction, id).await?;
        transaction.commit().await?;
        Ok(observation.filter(|observation| observation.project_id == project_id))
    }
}

async fn insert_root(
    connection: &mut SqliteConnection,
    observation_id: &RepositoryObservationId,
    root: &RepositoryRootObservation,
) -> Result<(), RepositoryObservationStoreError> {
    let (head_state, head_oid, head_ref) = match &root.head {
        RepositoryHead::Commit { oid, head_ref } => ("commit", Some(oid), head_ref.as_ref()),
        RepositoryHead::Unborn { head_ref } => ("unborn", None, head_ref.as_ref()),
        RepositoryHead::Unknown { .. } => ("unknown", None, None),
    };
    let (worktree_state, dirty_coverage, dirty_digest) = match &root.worktree {
        RepositoryWorktree::Clean => ("clean", "complete", None),
        RepositoryWorktree::Dirty {
            coverage: RepositoryDirtyCoverage::Complete { digest },
        } => ("dirty", "complete", Some(digest)),
        RepositoryWorktree::Dirty {
            coverage: RepositoryDirtyCoverage::Unknown { .. },
        } => ("dirty", "unknown", None),
        RepositoryWorktree::Unknown { .. } => ("unknown", "unknown", None),
    };
    sqlx::query(
        "INSERT INTO repository_root_observations (
            observation_id, project_root, git_worktree_root, head_state, head_oid, head_ref,
            worktree_state, dirty_digest, dirty_coverage, unknown_reason
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(observation_id.as_str())
    .bind(&root.project_root)
    .bind(&root.git_worktree_root)
    .bind(head_state)
    .bind(head_oid)
    .bind(head_ref)
    .bind(worktree_state)
    .bind(dirty_digest)
    .bind(dirty_coverage)
    .bind(root.unknown_reason()?.map(RepositoryUnknownReason::as_str))
    .execute(connection)
    .await?;
    Ok(())
}

async fn load_observation(
    connection: &mut SqliteConnection,
    id: &RepositoryObservationId,
) -> Result<Option<RepositoryObservation>, RepositoryObservationStoreError> {
    let Some(stored) = sqlx::query_as::<_, StoredObservation>(
        "SELECT project_id, format_version, started_at_ms, completed_at_ms, roots_digest,
                roots_coverage, omitted_root_count
         FROM repository_observations WHERE id = ?",
    )
    .bind(id.as_str())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };
    let roots = sqlx::query_as::<_, StoredRoot>(
        "SELECT project_root, git_worktree_root, head_state, head_oid, head_ref,
                worktree_state, dirty_digest, dirty_coverage, unknown_reason
         FROM repository_root_observations WHERE observation_id = ?
         ORDER BY project_root",
    )
    .bind(id.as_str())
    .fetch_all(&mut *connection)
    .await?;
    let corrupt = || RepositoryObservationStoreError::CorruptObservation(id.to_string());
    if stored.format_version != FORMAT_VERSION {
        return Err(corrupt());
    }
    let roots_coverage = match stored.roots_coverage.as_str() {
        "complete" => RepositoryRootsCoverage::Complete,
        "unknown" => RepositoryRootsCoverage::Unknown,
        _ => return Err(corrupt()),
    };
    let observation = RepositoryObservation {
        id: id.clone(),
        project_id: stored.project_id,
        started_at_ms: stored.started_at_ms,
        completed_at_ms: stored.completed_at_ms,
        roots_digest: stored.roots_digest,
        roots_coverage,
        omitted_root_count: u32::try_from(stored.omitted_root_count).map_err(|_| corrupt())?,
        roots: roots
            .into_iter()
            .map(|root| decode_root(root).ok_or_else(corrupt))
            .collect::<Result<_, _>>()?,
    };
    observation.validate().map_err(|_| corrupt())?;
    Ok(Some(observation))
}

fn decode_root(root: StoredRoot) -> Option<RepositoryRootObservation> {
    let reason = match root.unknown_reason.as_deref() {
        Some(reason) => Some(RepositoryUnknownReason::parse(reason)?),
        None => None,
    };
    let head = match (root.head_state.as_str(), root.head_oid) {
        ("commit", Some(oid)) => RepositoryHead::Commit {
            oid,
            head_ref: root.head_ref,
        },
        ("unborn", None) => RepositoryHead::Unborn {
            head_ref: root.head_ref,
        },
        ("unknown", None) => RepositoryHead::Unknown { reason: reason? },
        _ => return None,
    };
    let worktree = match (
        root.worktree_state.as_str(),
        root.dirty_coverage.as_str(),
        root.dirty_digest,
    ) {
        ("clean", "complete", None) => RepositoryWorktree::Clean,
        ("dirty", "complete", Some(digest)) => RepositoryWorktree::Dirty {
            coverage: RepositoryDirtyCoverage::Complete { digest },
        },
        ("dirty", "unknown", None) => RepositoryWorktree::Dirty {
            coverage: RepositoryDirtyCoverage::Unknown { reason: reason? },
        },
        ("unknown", "unknown", None) => RepositoryWorktree::Unknown { reason: reason? },
        _ => return None,
    };
    Some(RepositoryRootObservation {
        project_root: root.project_root,
        git_worktree_root: root.git_worktree_root,
        head,
        worktree,
    })
}

#[derive(Debug, Error)]
pub enum RepositoryObservationStoreError {
    #[error(transparent)]
    InvalidObservation(#[from] RepositoryObservationError),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("repository observation ID was already used for different data: {0}")]
    IdentityConflict(String),
    #[error("stored repository observation is corrupt: {0}")]
    CorruptObservation(String),
}

#[cfg(test)]
#[path = "repository_observation_storage_tests.rs"]
mod tests;
