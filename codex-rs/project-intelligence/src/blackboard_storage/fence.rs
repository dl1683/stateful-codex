use std::time::Duration;

use super::BlackboardStoreError;
use super::query::agent_knowledge_changed_since_on_connection;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use sqlx::pool::PoolConnection;

/// A database-wide SQLite writer fence held across completion validation and persistence,
/// or across the recording of a memory-bearing tool result into model history.
#[must_use]
pub struct CompletionFence {
    connection: PoolConnection<Sqlite>,
    released: bool,
}

impl CompletionFence {
    pub(super) async fn acquire(
        pool: &SqlitePool,
        timeout: Duration,
    ) -> Result<Self, BlackboardStoreError> {
        let mut connection = tokio::time::timeout(timeout, pool.acquire())
            .await
            .map_err(|_| BlackboardStoreError::CompletionFenceTimeout)??;
        match tokio::time::timeout(
            timeout,
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *connection),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                connection.close_on_drop();
                return Err(error.into());
            }
            Err(_) => {
                connection.close_on_drop();
                return Err(BlackboardStoreError::CompletionFenceTimeout);
            }
        }
        Ok(Self {
            connection,
            released: false,
        })
    }

    /// Whether any agent wrote project knowledge at or after `since_ms`, read while the
    /// fence holds the writer lock so no write can land between this check and the
    /// completion it guards.
    pub async fn agent_knowledge_changed_since(
        &mut self,
        project_id: &str,
        since_ms: i64,
    ) -> Result<bool, BlackboardStoreError> {
        agent_knowledge_changed_since_on_connection(&mut self.connection, project_id, since_ms)
            .await
    }

    /// The project's retirement generation (see `BlackboardStore::retirement_generation`),
    /// read while the fence holds the writer lock: no Forget, Undo or correction can commit
    /// until the fence is released or dropped.
    pub async fn retirement_generation(
        &mut self,
        project_id: &str,
    ) -> Result<u64, BlackboardStoreError> {
        super::source_group_read::retirement_generation_on(&mut self.connection, project_id).await
    }

    pub async fn release(mut self) -> Result<(), BlackboardStoreError> {
        sqlx::query("COMMIT").execute(&mut *self.connection).await?;
        self.released = true;
        Ok(())
    }
}

impl Drop for CompletionFence {
    /// A fence dropped without `release` (an early return or a failed commit) must not
    /// return its connection to the pool with the writer lock still held; closing the
    /// connection rolls the open transaction back and frees the lock.
    fn drop(&mut self) {
        if !self.released {
            self.connection.close_on_drop();
        }
    }
}
