//! Storage of investigation scopes and the threads bound to them.

use sqlx::FromRow;

use crate::ChangeRecord;
use crate::KnowledgeScope;
use crate::ScopeState;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::knowledge::append_change;
use super::knowledge::parse;

impl BlackboardStore {
    /// Opens `scope` unless a scope with its ID exists; returns the stored scope.
    pub async fn open_scope(
        &self,
        scope: &KnowledgeScope,
    ) -> Result<KnowledgeScope, BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO knowledge_scopes (
                project_id, scope_id, kind, title, state, end_condition, opened_source,
                ended_source, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, 'open', ?, ?, NULL, ?, ?)
             ON CONFLICT(project_id, scope_id) DO NOTHING",
        )
        .bind(&scope.project_id)
        .bind(&scope.scope_id)
        .bind(scope.kind.as_str())
        .bind(&scope.title)
        .bind(&scope.end_condition)
        .bind(&scope.opened_source)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.scope(&scope.project_id, &scope.scope_id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(scope.scope_id.clone()))
    }

    pub async fn scope(
        &self,
        project_id: &str,
        scope_id: &str,
    ) -> Result<Option<KnowledgeScope>, BlackboardStoreError> {
        sqlx::query_as::<_, StoredScope>(
            "SELECT * FROM knowledge_scopes WHERE project_id = ? AND scope_id = ?",
        )
        .bind(project_id)
        .bind(scope_id)
        .fetch_optional(&self.pool)
        .await?
        .map(StoredScope::into_scope)
        .transpose()
    }

    /// Scopes of the project, newest first, optionally only those in `state`.
    pub async fn scopes(
        &self,
        project_id: &str,
        state: Option<ScopeState>,
    ) -> Result<Vec<KnowledgeScope>, BlackboardStoreError> {
        sqlx::query_as::<_, StoredScope>(
            "SELECT * FROM knowledge_scopes
             WHERE project_id = ? AND (? IS NULL OR state = ?)
             ORDER BY created_at_ms DESC, scope_id",
        )
        .bind(project_id)
        .bind(state.map(ScopeState::as_str))
        .bind(state.map(ScopeState::as_str))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(StoredScope::into_scope)
        .collect()
    }

    /// Ends an open scope, journaling `change`. Ending an ended scope changes nothing and
    /// returns false.
    pub async fn end_scope(
        &self,
        project_id: &str,
        scope_id: &str,
        ended_source: &str,
        change: &ChangeRecord,
    ) -> Result<bool, BlackboardStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = unix_timestamp_millis()?;
        let ended = sqlx::query(
            "UPDATE knowledge_scopes SET state = 'ended', ended_source = ?, updated_at_ms = ?
             WHERE project_id = ? AND scope_id = ? AND state = 'open'",
        )
        .bind(ended_source)
        .bind(now)
        .bind(project_id)
        .bind(scope_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            == 1;
        if ended {
            append_change(&mut transaction, project_id, None, change, now).await?;
        }
        transaction.commit().await?;
        Ok(ended)
    }

    /// Binds a thread to a scope (replacing an earlier binding).
    pub async fn bind_thread_scope(
        &self,
        project_id: &str,
        thread_id: &str,
        scope_id: &str,
    ) -> Result<(), BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT INTO knowledge_scope_bindings (project_id, thread_id, scope_id, bound_at_ms)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(project_id, thread_id) DO UPDATE
             SET scope_id = excluded.scope_id, bound_at_ms = excluded.bound_at_ms",
        )
        .bind(project_id)
        .bind(thread_id)
        .bind(scope_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The scope a thread is bound to, if any.
    pub async fn thread_scope(
        &self,
        project_id: &str,
        thread_id: &str,
    ) -> Result<Option<KnowledgeScope>, BlackboardStoreError> {
        sqlx::query_as::<_, StoredScope>(
            "SELECT scope.* FROM knowledge_scope_bindings AS binding
             JOIN knowledge_scopes AS scope
               ON scope.project_id = binding.project_id AND scope.scope_id = binding.scope_id
             WHERE binding.project_id = ? AND binding.thread_id = ?",
        )
        .bind(project_id)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?
        .map(StoredScope::into_scope)
        .transpose()
    }

    /// Removes a thread's binding (the thread no longer continues any investigation).
    pub async fn unbind_thread_scope(
        &self,
        project_id: &str,
        thread_id: &str,
    ) -> Result<bool, BlackboardStoreError> {
        Ok(sqlx::query(
            "DELETE FROM knowledge_scope_bindings WHERE project_id = ? AND thread_id = ?",
        )
        .bind(project_id)
        .bind(thread_id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }
}

#[derive(FromRow)]
struct StoredScope {
    project_id: String,
    scope_id: String,
    kind: String,
    title: String,
    state: String,
    end_condition: Option<String>,
    opened_source: String,
    ended_source: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl StoredScope {
    fn into_scope(self) -> Result<KnowledgeScope, BlackboardStoreError> {
        Ok(KnowledgeScope {
            project_id: self.project_id,
            scope_id: self.scope_id,
            kind: parse(&self.kind)?,
            title: self.title,
            state: parse(&self.state)?,
            end_condition: self.end_condition,
            opened_source: self.opened_source,
            ended_source: self.ended_source,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        })
    }
}
