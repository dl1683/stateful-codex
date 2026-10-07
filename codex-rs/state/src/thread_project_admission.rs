//! Cross-store operations validate a thread's project while holding metadata writer admission.

use crate::SqliteConfig;
use codex_protocol::ThreadId;
use sqlx::Sqlite;
use sqlx::SqlitePool;
use sqlx::Transaction;

/// Keeps the authoritative project binding unchanged until a dependent operation finishes.
/// Dropping the guard rolls back the read-only writer transaction and releases admission.
pub struct ThreadProjectAdmission {
    _transaction: Transaction<'static, Sqlite>,
    _pool: SqlitePool,
}

#[cfg(test)]
#[path = "thread_project_admission_tests.rs"]
mod tests;

impl ThreadProjectAdmission {
    /// Refuses missing/unpersisted or mismatched bindings. Independent metadata writers
    /// cannot rebind or unlink this thread while the returned guard is alive.
    pub async fn acquire(
        sqlite: &SqliteConfig,
        thread_id: ThreadId,
        expected_project_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        let pool = sqlite.open_read_write_pool(&sqlite.state_db_path()).await?;
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
        let project =
            sqlx::query_scalar::<_, Option<String>>("SELECT project_id FROM threads WHERE id = ?")
                .bind(thread_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?
                .flatten();
        if expected_project_id.is_empty() || project.as_deref() != Some(expected_project_id) {
            return Ok(None);
        }
        Ok(Some(Self {
            _transaction: transaction,
            _pool: pool,
        }))
    }
}
