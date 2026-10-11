//! Derived working-set capsule storage: the per-project eligibility generation, bounded
//! projections and file samples keyed by project/binding generation/thread, and the
//! publication ticket contract. Nothing here is authority; every row is rebuildable.

use codex_state::SqliteConfig;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use sqlx::Sqlite;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use sqlx::query::Query;
use sqlx::sqlite::SqliteArguments;
use thiserror::Error;

use crate::BlackboardStoreError;
use crate::storage::DATABASE_NAME;
use crate::storage::HierarchyStoreError;
use crate::storage::unix_timestamp_millis;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
pub const MAX_WORKING_SET_PAYLOAD_BYTES: usize = 128 * 1024;
pub const MAX_WORKING_SET_FILES: usize = 8;
pub const MAX_WORKING_SET_FILE_BYTES: usize = 64 * 1024;
const MAX_EVENTS_PER_UPDATE: usize = 1024;

/// Identity of one thread's derived view. A run is never key material, so Ask threads work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingSetKey {
    pub project_id: String,
    pub binding_generation: u64,
    pub thread_id: String,
    pub projection_version: u32,
}

/// Last contiguous accepted host event plus a chained digest of that source prefix.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceWatermark {
    pub sequence: u64,
    pub digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingSetSourceEvent {
    pub sequence: u64,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSetInvalidation {
    Retirement,
    SourceRevision,
    Scope,
    Binding,
    Archive,
    SourceGap,
    Version,
    Corrupt,
}

/// Appends contiguous events to the projection at `expected_revision`; 0 creates it, or
/// rebuilds an invalidated one from the genesis watermark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingSetUpdate {
    pub key: WorkingSetKey,
    pub run_id: Option<String>,
    pub expected_revision: u64,
    pub prepared_generation: u64,
    pub events: Vec<WorkingSetSourceEvent>,
    pub payload: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingSetProjection {
    pub key: WorkingSetKey,
    pub run_id: Option<String>,
    pub revision: u64,
    pub watermark: SourceWatermark,
    pub eligibility_generation: u64,
    pub payload: String,
    pub invalidation: Option<WorkingSetInvalidation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSampleCoverage {
    Complete,
    Truncated,
    Binary,
    Oversized,
    Unreadable,
    OutOfRoot,
}

/// Bounded content sample of one task-touched file read through its executor filesystem.
/// A stored sample whose content no longer matches its digest reads back `Unreadable`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingSetFileSample {
    pub executor_id: String,
    pub root: String,
    pub relative_path: String,
    pub sample_event: String,
    pub coverage: FileSampleCoverage,
    pub content: Option<String>,
}

/// One coherent read; publication must present this generation back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingSetView {
    pub eligibility_generation: u64,
    pub projection: Option<WorkingSetProjection>,
    pub files: Vec<WorkingSetFileSample>,
}

#[derive(Clone)]
pub struct WorkingSetStore {
    pool: SqlitePool,
}

type FileRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

impl WorkingSetStore {
    pub async fn open(sqlite: &SqliteConfig) -> Result<Self, WorkingSetStoreError> {
        let path = sqlite.home().join(DATABASE_NAME);
        sqlite.check_migration_history(&path, &MIGRATOR).await?;
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let pool = sqlite.open_read_write_pool(&path).await?;
        if let Err(error) = sqlite.run_migrations(&pool, &MIGRATOR).await {
            pool.close().await;
            return Err(error.into());
        }
        Ok(Self { pool })
    }

    pub async fn eligibility_generation(&self, project: &str) -> Result<u64, WorkingSetStoreError> {
        generation_on(&mut *self.pool.acquire().await?, project).await
    }

    /// Reads generation, projection and file samples in one read transaction.
    pub async fn view(&self, key: &WorkingSetKey) -> Result<WorkingSetView, WorkingSetStoreError> {
        validate_key(key)?;
        let mut tx = self.pool.begin().await?;
        let eligibility_generation = generation_on(&mut tx, &key.project_id).await?;
        let projection = load_projection_on(&mut tx, key).await?;
        let rows: Vec<FileRow> = sqlx::query_as(
            "SELECT executor_id, root, relative_path, sample_event, coverage, content,
                    content_digest FROM working_set_file_samples
             WHERE project_id = ? AND binding_generation = ? AND thread_id = ?
             ORDER BY executor_id, root, relative_path LIMIT 8",
        )
        .bind(&key.project_id)
        .bind(to_i64(key.binding_generation)?)
        .bind(&key.thread_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut files = Vec::with_capacity(rows.len());
        for (executor_id, root, relative_path, sample_event, coverage, content, stored) in rows {
            let intact = content.as_deref().map(digest) == stored;
            files.push(WorkingSetFileSample {
                executor_id,
                root,
                relative_path,
                sample_event,
                coverage: if intact {
                    parse(&coverage)?
                } else {
                    FileSampleCoverage::Unreadable
                },
                content: content.filter(|_| intact),
            });
        }
        Ok(WorkingSetView {
            eligibility_generation,
            projection,
            files,
        })
    }

    /// Advances the projection only across contiguous source events, and only when no
    /// capsule-relevant eligibility changed since the caller prepared it.
    pub async fn write_projection(
        &self,
        update: WorkingSetUpdate,
    ) -> Result<WorkingSetProjection, WorkingSetStoreError> {
        let key = &update.key;
        validate_key(key)?;
        if update.payload.len() > MAX_WORKING_SET_PAYLOAD_BYTES
            || update.events.len() > MAX_EVENTS_PER_UPDATE
            || update
                .events
                .iter()
                .any(|event| !bounded(&event.digest, 128))
            || update
                .run_id
                .as_deref()
                .is_some_and(|run| !bounded(run, 512))
        {
            return Err(WorkingSetStoreError::InvalidRequest("projection bounds"));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = generation_on(&mut tx, &key.project_id).await?;
        if current != update.prepared_generation {
            let prepared = update.prepared_generation;
            return Err(WorkingSetStoreError::EligibilityChanged { prepared, current });
        }
        let (revision, mut watermark) = match load_projection_on(&mut tx, key).await? {
            None if update.expected_revision == 0 => (0, SourceWatermark::default()),
            Some(row) if row.invalidation.is_some() && update.expected_revision == 0 => {
                (row.revision, SourceWatermark::default())
            }
            Some(WorkingSetProjection {
                invalidation: Some(reason),
                ..
            }) => {
                return Err(WorkingSetStoreError::Invalidated(reason));
            }
            Some(row) if row.revision == update.expected_revision => (row.revision, row.watermark),
            row => {
                let expected = update.expected_revision;
                let actual = row.map_or(0, |row| row.revision);
                return Err(WorkingSetStoreError::RevisionConflict { expected, actual });
            }
        };
        for event in &update.events {
            let expected = watermark.sequence + 1;
            if event.sequence != expected {
                let found = event.sequence;
                return Err(WorkingSetStoreError::WatermarkGap { expected, found });
            }
            let chained = format!("{}\n{}\n{}", watermark.digest, event.sequence, event.digest);
            watermark = SourceWatermark {
                sequence: event.sequence,
                digest: digest(&chained),
            };
        }
        keyed(
            "INSERT OR REPLACE INTO working_set_projections VALUES
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12)",
            key,
        )?
        .bind(i64::from(key.projection_version))
        .bind(&update.run_id)
        .bind(to_i64(revision + 1)?)
        .bind(to_i64(watermark.sequence)?)
        .bind(&watermark.digest)
        .bind(to_i64(current)?)
        .bind(&update.payload)
        .bind(digest(&update.payload))
        .bind(unix_timestamp_millis()?)
        .execute(&mut *tx)
        .await?;
        // Older bindings and projection versions of this thread are obsolete derived state.
        keyed(
            "DELETE FROM working_set_projections WHERE project_id = ?1 AND thread_id = ?3
             AND (binding_generation < ?2 OR (binding_generation = ?2 AND projection_version != ?4))",
            key,
        )?
        .bind(i64::from(key.projection_version))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(WorkingSetProjection {
            key: key.clone(),
            run_id: update.run_id,
            revision: revision + 1,
            watermark,
            eligibility_generation: current,
            payload: update.payload,
            invalidation: None,
        })
    }

    /// Stores the latest bounded sample for one of at most eight files of the thread.
    pub async fn record_file_sample(
        &self,
        key: &WorkingSetKey,
        sample: &WorkingSetFileSample,
    ) -> Result<(), WorkingSetStoreError> {
        validate_key(key)?;
        let sampled = matches!(
            sample.coverage,
            FileSampleCoverage::Complete | FileSampleCoverage::Truncated
        );
        if !bounded(&sample.executor_id, 512)
            || !bounded(&sample.root, 4096)
            || !bounded(&sample.relative_path, 4096)
            || !bounded(&sample.sample_event, 512)
            || sampled != sample.content.is_some()
            || sample
                .content
                .as_ref()
                .is_some_and(|text| text.len() > MAX_WORKING_SET_FILE_BYTES)
        {
            return Err(WorkingSetStoreError::InvalidRequest("file sample bounds"));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (count, present): (i64, bool) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(MAX(executor_id = ?4 AND root = ?5 AND relative_path = ?6), 0)
             FROM working_set_file_samples
             WHERE project_id = ?1 AND binding_generation = ?2 AND thread_id = ?3",
        )
        .bind(&key.project_id)
        .bind(to_i64(key.binding_generation)?)
        .bind(&key.thread_id)
        .bind(&sample.executor_id)
        .bind(&sample.root)
        .bind(&sample.relative_path)
        .fetch_one(&mut *tx)
        .await?;
        if !present && count >= MAX_WORKING_SET_FILES as i64 {
            return Err(WorkingSetStoreError::FileLimit);
        }
        keyed(
            "INSERT OR REPLACE INTO working_set_file_samples VALUES
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            key,
        )?
        .bind(&sample.executor_id)
        .bind(&sample.root)
        .bind(&sample.relative_path)
        .bind(&sample.sample_event)
        .bind(name(sample.coverage))
        .bind(&sample.content)
        .bind(sample.content.as_deref().map(digest))
        .bind(unix_timestamp_millis()?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Starts a statement whose first three parameters are the key's project, binding, thread.
fn keyed(
    sql: &'static str,
    key: &WorkingSetKey,
) -> Result<Query<'static, Sqlite, SqliteArguments>, WorkingSetStoreError> {
    Ok(sqlx::query(sql)
        .bind(key.project_id.clone())
        .bind(to_i64(key.binding_generation)?)
        .bind(key.thread_id.clone()))
}

async fn generation_on(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<u64, WorkingSetStoreError> {
    let generation: Option<i64> = sqlx::query_scalar(
        "SELECT generation FROM capsule_eligibility_generations WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(connection)
    .await?;
    from_i64(generation.unwrap_or(0))
}

/// A row whose payload no longer matches its digest reads back as `Corrupt` and empty.
async fn load_projection_on(
    connection: &mut SqliteConnection,
    key: &WorkingSetKey,
) -> Result<Option<WorkingSetProjection>, WorkingSetStoreError> {
    type Row = (
        Option<String>,
        i64,
        i64,
        String,
        i64,
        String,
        String,
        Option<String>,
    );
    let row: Option<Row> = keyed(
        "SELECT run_id, projection_revision, source_watermark, source_digest,
                eligibility_generation, payload, payload_digest, invalidation
         FROM working_set_projections WHERE project_id = ?1 AND binding_generation = ?2
           AND thread_id = ?3 AND projection_version = ?4",
        key,
    )?
    .bind(i64::from(key.projection_version))
    .try_map(|row| sqlx::FromRow::from_row(&row))
    .fetch_optional(connection)
    .await?;
    let Some((run_id, revision, sequence, source, generation, payload, stored, reason)) = row
    else {
        return Ok(None);
    };
    let intact = digest(&payload) == stored;
    Ok(Some(WorkingSetProjection {
        key: key.clone(),
        run_id,
        revision: from_i64(revision)?,
        watermark: SourceWatermark {
            sequence: from_i64(sequence)?,
            digest: source,
        },
        eligibility_generation: from_i64(generation)?,
        payload: if intact { payload } else { String::new() },
        invalidation: match reason {
            Some(reason) => Some(parse(&reason)?),
            None => (!intact).then_some(WorkingSetInvalidation::Corrupt),
        },
    }))
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max
}

fn validate_key(key: &WorkingSetKey) -> Result<(), WorkingSetStoreError> {
    let valid = bounded(&key.project_id, 512)
        && bounded(&key.thread_id, 512)
        && key.binding_generation > 0
        && key.projection_version > 0;
    valid
        .then_some(())
        .ok_or(WorkingSetStoreError::InvalidRequest("working-set key"))
}

fn to_i64(value: u64) -> Result<i64, WorkingSetStoreError> {
    i64::try_from(value).map_err(|_| WorkingSetStoreError::InvalidRequest("integer overflow"))
}

fn from_i64(value: i64) -> Result<u64, WorkingSetStoreError> {
    u64::try_from(value).map_err(|_| WorkingSetStoreError::Corrupt(value.to_string()))
}

/// Stored enum spelling; the migration's CHECK constraints list the same snake_case names.
fn name(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn parse<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, WorkingSetStoreError> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|_| WorkingSetStoreError::Corrupt(value.to_string()))
}

#[derive(Debug, Error)]
pub enum WorkingSetStoreError {
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Eligibility(#[from] BlackboardStoreError),
    #[error(transparent)]
    Clock(#[from] HierarchyStoreError),
    #[error("working-set request is out of bounds: {0}")]
    InvalidRequest(&'static str),
    #[error("capsule eligibility changed: prepared {prepared}, current {current}")]
    EligibilityChanged { prepared: u64, current: u64 },
    #[error("working-set revision conflict: expected {expected}, found {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("source watermark gap: expected event {expected}, found {found}")]
    WatermarkGap { expected: u64, found: u64 },
    #[error("working-set projection is invalidated: {0:?}")]
    Invalidated(WorkingSetInvalidation),
    #[error("working-set file sample limit reached")]
    FileLimit,
    #[error("stored working-set value is corrupt: {0}")]
    Corrupt(String),
}

#[cfg(test)]
#[path = "working_set_storage_tests.rs"]
mod tests;
