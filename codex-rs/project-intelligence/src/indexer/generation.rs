use sqlx::SqliteConnection;

use super::IndexCancellation;
use super::ProjectIndexer;
use super::ProjectIndexerError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RefreshGeneration(i64);

pub(super) async fn claim(
    indexer: &ProjectIndexer,
    project_id: &str,
) -> Result<RefreshGeneration, ProjectIndexerError> {
    let mut transaction = indexer.context_map.begin_immediate().await?;
    let generation = sqlx::query_scalar::<_, i64>(
        "INSERT INTO project_index_refresh_generation (project_id, latest_generation)
         VALUES (?, 1)
         ON CONFLICT(project_id) DO UPDATE SET
            latest_generation = latest_generation + 1
         RETURNING latest_generation",
    )
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(RefreshGeneration(generation))
}

pub(super) async fn require_current(
    connection: &mut SqliteConnection,
    project_id: &str,
    expected: RefreshGeneration,
) -> Result<(), ProjectIndexerError> {
    let actual = sqlx::query_scalar::<_, i64>(
        "SELECT latest_generation FROM project_index_refresh_generation
         WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(connection)
    .await?;
    if actual == Some(expected.0) {
        return Ok(());
    }
    Err(ProjectIndexerError::SupersededRefresh)
}

/// What every publication transaction of one index operation checks under the writer
/// lock before it commits: the operation was not cancelled and is still the newest one.
#[derive(Clone, Copy)]
pub(super) struct PublicationFence<'a> {
    pub(super) generation: RefreshGeneration,
    pub(super) cancellation: &'a IndexCancellation,
}

impl PublicationFence<'_> {
    pub(super) async fn check(
        &self,
        connection: &mut SqliteConnection,
        project_id: &str,
    ) -> Result<(), ProjectIndexerError> {
        self.cancellation.check()?;
        require_current(connection, project_id, self.generation).await
    }
}
