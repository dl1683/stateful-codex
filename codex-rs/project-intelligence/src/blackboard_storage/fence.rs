use std::time::Duration;

use super::AgentKnowledgeChange;
use super::BlackboardStoreError;
use super::query::agent_knowledge_for_run_on_connection;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use sqlx::pool::PoolConnection;

/// A database-wide SQLite writer fence held across completion validation and persistence.
#[must_use]
pub struct CompletionFence {
    connection: PoolConnection<Sqlite>,
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
        Ok(Self { connection })
    }

    pub async fn agent_knowledge_for_run(
        &mut self,
        project_id: &str,
        agent_run_id: &str,
        run_created_at_ms: i64,
    ) -> Result<AgentKnowledgeChange, BlackboardStoreError> {
        agent_knowledge_for_run_on_connection(
            &mut self.connection,
            project_id,
            agent_run_id,
            run_created_at_ms,
        )
        .await
    }

    pub async fn release(mut self) -> Result<(), BlackboardStoreError> {
        sqlx::query("COMMIT").execute(&mut *self.connection).await?;
        Ok(())
    }
}
