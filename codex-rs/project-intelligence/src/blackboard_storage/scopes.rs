//! Exact historical scope lookup and preserved quarantine predicates.

use sqlx::FromRow;

use crate::KnowledgeScope;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::knowledge::parse;

/// Leaves out an entry (`entry` in the enclosing query) whose current meaning limits it to
/// an investigation other than the open one the thread (the bound parameter) continues.
pub(super) const OUTSIDE_THREAD_SCOPE: &str = "
               AND NOT EXISTS (
                   SELECT 1 FROM knowledge_context AS scoped
                   WHERE scoped.entry_id = entry.id
                     AND scoped.revision = (
                         SELECT MAX(latest.revision) FROM knowledge_context AS latest
                         WHERE latest.entry_id = entry.id AND latest.revision <= entry.revision)
                     AND scoped.scope_id IS NOT NULL
                     AND scoped.scope_id IS NOT (
                         SELECT binding.scope_id FROM knowledge_scope_bindings AS binding
                         JOIN knowledge_scopes AS bound
                           ON bound.project_id = binding.project_id
                          AND bound.scope_id = binding.scope_id
                         WHERE binding.project_id = entry.project_id AND binding.thread_id = ?
                           AND bound.state = 'open'))";

/// An instruction recorded before meanings were kept (no context row) whose words limit it to
/// a piece of work ("for this investigation", "this bug", as historical capture recognized them): which
/// piece is unknown, so it applies nowhere until the user restates it.
pub(super) const LEGACY_LIMITED_RULE: &str = "revision.kind = 'instruction'
                   AND NOT EXISTS (
                       SELECT 1 FROM knowledge_context AS any_context
                       WHERE any_context.entry_id = entry.id)
                   AND (LOWER(revision.content) LIKE '%investigation%'
                        OR LOWER(revision.content) LIKE '%this bug%'
                        OR LOWER(revision.content) LIKE '%this issue%'
                        OR LOWER(revision.content) LIKE '%this incident%'
                        OR LOWER(revision.content) LIKE '%this debugging%')";

impl BlackboardStore {
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
