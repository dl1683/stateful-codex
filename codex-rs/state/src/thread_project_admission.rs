//! Cross-store operations validate a thread's project while holding metadata writer admission.

use crate::SqliteConfig;
use codex_protocol::ThreadId;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use sqlx::Transaction;

/// Keeps the authoritative project binding unchanged until a dependent operation finishes.
/// Dropping the guard rolls back the read-only writer transaction and releases admission.
pub struct ThreadProjectAdmission {
    binding_generation: u64,
    thread_id: String,
    project_id: String,
    _transaction: Transaction<'static, Sqlite>,
    _pool: SqlitePool,
}

#[cfg(test)]
#[path = "thread_project_admission_tests.rs"]
mod tests;

impl ThreadProjectAdmission {
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    /// Durable generation changes on every link, unlink or project switch, including A→B→A.
    pub fn binding_generation(&self) -> u64 {
        self.binding_generation
    }
    /// Refuses missing/unpersisted or mismatched bindings. Independent metadata writers
    /// cannot rebind or unlink this thread while the returned guard is alive.
    pub async fn acquire(
        sqlite: &SqliteConfig,
        thread_id: ThreadId,
        expected_project_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
        let binding = sqlx::query_as::<_, (Option<String>, i64)>(
            "SELECT project_id, project_binding_generation FROM threads WHERE id = ?",
        )
        .bind(thread_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((project, generation)) = binding else {
            return Ok(None);
        };
        if expected_project_id.is_empty() || project.as_deref() != Some(expected_project_id) {
            return Ok(None);
        }
        Ok(Some(Self {
            thread_id: thread_id.to_string(),
            project_id: expected_project_id.to_string(),
            binding_generation: u64::try_from(generation).map_err(|_| {
                sqlx::Error::Protocol("invalid project binding generation".to_string())
            })?,
            _transaction: transaction,
            _pool: pool,
        }))
    }
}
