//! Stable source positions allocated inside the PI writer transaction.

use super::BlackboardStoreError;

/// Allocates the next source position while the caller holds the PI writer transaction.
pub(super) async fn allocate_on(
    connection: &mut sqlx::SqliteConnection,
    project_id: &str,
) -> Result<u64, BlackboardStoreError> {
    let next: i64 = sqlx::query_scalar(
        "INSERT INTO knowledge_source_sequences (project_id, next_sequence) VALUES (?, 2)
         ON CONFLICT(project_id) DO UPDATE SET next_sequence = next_sequence + 1
         RETURNING next_sequence - 1",
    )
    .bind(project_id)
    .fetch_one(connection)
    .await?;
    u64::try_from(next).map_err(|_| BlackboardStoreError::RevisionOverflow)
}
