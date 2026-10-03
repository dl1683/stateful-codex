//! Storage of entry context, scopes, capture groups and the memory-change journal. Context and
//! journal rows are written in the same transaction as the entry revision they describe.

use std::collections::HashMap;

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::CaptureGroup;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::KnowledgeScope;
use crate::MAX_CHANGE_PREVIEW_BYTES;
use crate::MemoryChange;
use crate::NewBlackboardEntry;
use crate::ScopeState;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::insert_new_entry;
use super::load_entry;
use super::load_entry_by_id;

/// Largest journal page returned at once.
pub const MAX_CHANGES_PAGE: u32 = 500;

/// Whether a write created the entry or found the same entry already stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateOutcome {
    Created,
    AlreadyPresent,
}

impl BlackboardStore {
    /// Creates `id` with its context and journals the save, all in one transaction. The same
    /// value already stored under `id` is returned unchanged, with no journal row.
    pub async fn create_entry_with_context(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
        context: KnowledgeContext,
        change: ChangeRecord,
    ) -> Result<(BlackboardEntry, CreateOutcome), BlackboardStoreError> {
        value.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_entry_by_id(&mut transaction, &id).await? {
            if existing.value != value
                || existing.state != BlackboardEntryState::Active
                || existing.superseded_by.is_some()
            {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
            return Ok((existing, CreateOutcome::AlreadyPresent));
        }
        let now = unix_timestamp_millis()?;
        insert_new_entry(&mut transaction, &id, &value, now).await?;
        write_context(&mut transaction, &value.project_id, &id, 1, &context).await?;
        append_change(
            &mut transaction,
            &value.project_id,
            Some((&id, 1)),
            &change,
            now,
        )
        .await?;
        let entry = load_entry(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok((entry, CreateOutcome::Created))
    }

    /// The context in effect for the entry's current revision: the newest context recorded
    /// at or before it (lifecycle-only revisions keep their predecessor's).
    pub async fn knowledge_context(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
    ) -> Result<Option<KnowledgeContext>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        context_of(&mut connection, project_id, id.as_str()).await
    }

    /// Contexts of several entries, by entry ID; entries without one are legacy.
    pub async fn knowledge_contexts(
        &self,
        project_id: &str,
        ids: &[BlackboardEntryId],
    ) -> Result<HashMap<String, KnowledgeContext>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        let mut contexts = HashMap::with_capacity(ids.len());
        for id in ids {
            if let Some(context) = context_of(&mut connection, project_id, id.as_str()).await? {
                contexts.insert(id.to_string(), context);
            }
        }
        Ok(contexts)
    }

    /// Records `context` for an existing revision of an entry (a revision another writer
    /// created), journaling `change` in the same transaction.
    pub async fn record_context(
        &self,
        entry: &BlackboardEntry,
        context: &KnowledgeContext,
        change: Option<&ChangeRecord>,
    ) -> Result<(), BlackboardStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let revision =
            i64::try_from(entry.revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        write_context(
            &mut transaction,
            &entry.value.project_id,
            &entry.id,
            revision,
            context,
        )
        .await?;
        if let Some(change) = change {
            let now = unix_timestamp_millis()?;
            append_change(
                &mut transaction,
                &entry.value.project_id,
                Some((&entry.id, revision)),
                change,
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// The next position in the project's capture order.
    pub async fn allocate_source_sequence(
        &self,
        project_id: &str,
    ) -> Result<u64, BlackboardStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let next = sqlx::query_scalar::<_, i64>(
            "INSERT INTO knowledge_source_sequences (project_id, next_sequence) VALUES (?, 2)
             ON CONFLICT(project_id) DO UPDATE SET next_sequence = next_sequence + 1
             RETURNING next_sequence - 1",
        )
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        u64::try_from(next).map_err(|_| BlackboardStoreError::RevisionOverflow)
    }

    /// Journals a change that has no entry revision of its own (a scope ended, a capture that
    /// could not finish).
    pub async fn record_change(
        &self,
        project_id: &str,
        entry: Option<&BlackboardEntry>,
        change: &ChangeRecord,
    ) -> Result<u64, BlackboardStoreError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = unix_timestamp_millis()?;
        let revision = entry
            .map(|entry| i64::try_from(entry.revision))
            .transpose()
            .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        let sequence = append_change(
            &mut transaction,
            project_id,
            entry
                .zip(revision)
                .map(|(entry, revision)| (&entry.id, revision)),
            change,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(sequence)
    }

    /// Journal rows after `after`, oldest first, at most `limit`, optionally for one thread.
    pub async fn memory_changes(
        &self,
        project_id: &str,
        thread_id: Option<&str>,
        after: u64,
        limit: u32,
    ) -> Result<Vec<MemoryChange>, BlackboardStoreError> {
        let after = i64::try_from(after).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        let limit = i64::from(limit.clamp(1, MAX_CHANGES_PAGE));
        let rows = sqlx::query_as::<_, StoredChange>(
            "SELECT * FROM memory_changes
             WHERE project_id = ? AND sequence > ? AND (? IS NULL OR thread_id = ?)
             ORDER BY sequence LIMIT ?",
        )
        .bind(project_id)
        .bind(after)
        .bind(thread_id)
        .bind(thread_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(StoredChange::into_change).collect()
    }

    /// The newest journal sequence of the project (0 when nothing was journaled).
    pub async fn latest_change_sequence(
        &self,
        project_id: &str,
    ) -> Result<u64, BlackboardStoreError> {
        let sequence = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(sequence) FROM memory_changes WHERE project_id = ?",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await?
        .unwrap_or(0);
        u64::try_from(sequence).map_err(|_| BlackboardStoreError::RevisionOverflow)
    }

    /// Stores the outcome of one capture (replacing an earlier record of the same group).
    pub async fn record_capture_group(
        &self,
        group: &CaptureGroup,
    ) -> Result<(), BlackboardStoreError> {
        let now = unix_timestamp_millis()?;
        sqlx::query(
            "INSERT OR REPLACE INTO capture_groups (
                project_id, group_id, thread_id, turn_id, kind, declared_count, recognized,
                saved, already_present, pending, omitted, failed, recorded_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&group.project_id)
        .bind(&group.group_id)
        .bind(&group.thread_id)
        .bind(&group.turn_id)
        .bind(&group.kind)
        .bind(group.declared_count.map(i64::from))
        .bind(i64::from(group.recognized))
        .bind(i64::from(group.saved))
        .bind(i64::from(group.already_present))
        .bind(i64::from(group.pending))
        .bind(i64::from(group.omitted))
        .bind(i64::from(group.failed))
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

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
}

/// Inserts the context of one entry revision.
pub(super) async fn write_context(
    connection: &mut SqliteConnection,
    project_id: &str,
    id: &BlackboardEntryId,
    revision: i64,
    context: &KnowledgeContext,
) -> Result<(), BlackboardStoreError> {
    sqlx::query(
        "INSERT OR REPLACE INTO knowledge_context (
            entry_id, revision, project_id, category, authority, scope_id, end_condition,
            source_sequence, unit_ordinal, group_id, validity, payload
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.as_str())
    .bind(revision)
    .bind(project_id)
    .bind(context.category.as_str())
    .bind(context.authority.as_str())
    .bind(&context.scope_id)
    .bind(&context.end_condition)
    .bind(
        context
            .source_sequence
            .map(i64::try_from)
            .transpose()
            .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
    )
    .bind(context.unit_ordinal.map(i64::from))
    .bind(&context.group_id)
    .bind(context.validity.as_str())
    .bind(&context.payload)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// Copies the newest context of `from` to revision `revision` of `to` (a successor keeps its
/// predecessor's category, scope and position).
pub(super) async fn carry_context(
    connection: &mut SqliteConnection,
    project_id: &str,
    from: &BlackboardEntryId,
    to: &BlackboardEntryId,
    revision: i64,
) -> Result<(), BlackboardStoreError> {
    if let Some(context) = context_of(&mut *connection, project_id, from.as_str()).await? {
        write_context(connection, project_id, to, revision, &context).await?;
    }
    Ok(())
}

/// Appends one journal row and returns its sequence.
pub(super) async fn append_change(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry: Option<(&BlackboardEntryId, i64)>,
    change: &ChangeRecord,
    now: i64,
) -> Result<u64, BlackboardStoreError> {
    let preview = bounded_preview(&change.preview);
    let preview = preview.as_ref();
    let sequence = sqlx::query_scalar::<_, i64>(
        "INSERT INTO memory_changes (
            project_id, sequence, entry_id, revision, operation, origin, category, action_id,
            thread_id, turn_id, group_id, preview, created_at_ms
         ) VALUES (
            ?, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM memory_changes WHERE project_id = ?),
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
         ) RETURNING sequence",
    )
    .bind(project_id)
    .bind(project_id)
    .bind(entry.map(|(id, _)| id.as_str()))
    .bind(entry.map(|(_, revision)| revision))
    .bind(change.operation.as_str())
    .bind(change.origin.as_str())
    .bind(change.category.as_str())
    .bind(&change.action_id)
    .bind(&change.thread_id)
    .bind(&change.turn_id)
    .bind(&change.group_id)
    .bind(preview)
    .bind(now)
    .fetch_one(&mut *connection)
    .await?;
    u64::try_from(sequence).map_err(|_| BlackboardStoreError::RevisionOverflow)
}

/// `text` within `MAX_CHANGE_PREVIEW_BYTES`; a cut ends on a character boundary and is
/// marked with an ellipsis, so a cut sentence never reads as a whole one.
fn bounded_preview(text: &str) -> std::borrow::Cow<'_, str> {
    if text.len() <= MAX_CHANGE_PREVIEW_BYTES {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut end = MAX_CHANGE_PREVIEW_BYTES - '\u{2026}'.len_utf8();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    std::borrow::Cow::Owned(format!("{}\u{2026}", &text[..end]))
}

pub(super) async fn context_of(
    connection: &mut SqliteConnection,
    project_id: &str,
    entry_id: &str,
) -> Result<Option<KnowledgeContext>, BlackboardStoreError> {
    sqlx::query_as::<_, StoredContext>(
        "SELECT context.* FROM knowledge_context AS context
         JOIN blackboard_entries AS entry ON entry.id = context.entry_id
         WHERE context.project_id = ? AND context.entry_id = ?
           AND context.revision <= entry.revision
         ORDER BY context.revision DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(entry_id)
    .fetch_optional(&mut *connection)
    .await?
    .map(StoredContext::into_context)
    .transpose()
}

fn parse<T: std::str::FromStr<Err = String>>(value: &str) -> Result<T, BlackboardStoreError> {
    value
        .parse()
        .map_err(|error: String| BlackboardStoreError::InvalidStoredKnowledge(error))
}

fn unsigned(value: i64) -> Result<u64, BlackboardStoreError> {
    u64::try_from(value).map_err(|_| BlackboardStoreError::RevisionOverflow)
}

#[derive(FromRow)]
struct StoredContext {
    category: String,
    authority: String,
    scope_id: Option<String>,
    end_condition: Option<String>,
    source_sequence: Option<i64>,
    unit_ordinal: Option<i64>,
    group_id: Option<String>,
    validity: String,
    payload: Option<String>,
}

impl StoredContext {
    fn into_context(self) -> Result<KnowledgeContext, BlackboardStoreError> {
        Ok(KnowledgeContext {
            category: parse(&self.category)?,
            authority: parse(&self.authority)?,
            scope_id: self.scope_id,
            end_condition: self.end_condition,
            source_sequence: self.source_sequence.map(unsigned).transpose()?,
            unit_ordinal: self
                .unit_ordinal
                .map(|ordinal| {
                    u32::try_from(ordinal).map_err(|_| BlackboardStoreError::RevisionOverflow)
                })
                .transpose()?,
            group_id: self.group_id,
            validity: parse(&self.validity)?,
            payload: self.payload,
        })
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

#[derive(FromRow)]
pub(super) struct StoredChange {
    project_id: String,
    sequence: i64,
    entry_id: Option<String>,
    revision: Option<i64>,
    operation: String,
    origin: String,
    category: String,
    action_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    group_id: Option<String>,
    preview: String,
    created_at_ms: i64,
}

impl StoredChange {
    pub(super) fn into_change(self) -> Result<MemoryChange, BlackboardStoreError> {
        Ok(MemoryChange {
            project_id: self.project_id,
            sequence: unsigned(self.sequence)?,
            entry_id: self.entry_id,
            revision: self.revision.map(unsigned).transpose()?,
            record: ChangeRecord {
                operation: parse(&self.operation)?,
                origin: parse(&self.origin)?,
                category: parse(&self.category)?,
                action_id: self.action_id,
                thread_id: self.thread_id,
                turn_id: self.turn_id,
                group_id: self.group_id,
                preview: self.preview,
            },
            created_at_ms: self.created_at_ms,
        })
    }
}

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;
