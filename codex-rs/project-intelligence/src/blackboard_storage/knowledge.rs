//! Storage of entry context, scopes, capture groups and the memory-change journal. Context and
//! journal rows are written in the same transaction as the entry revision they describe.

use std::collections::HashMap;

use sqlx::FromRow;
use sqlx::SqliteConnection;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::CaptureGroup;
use crate::CaptureGroupMember;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::MAX_CHANGE_PREVIEW_BYTES;
use crate::MemoryChange;
use crate::NewBlackboardEntry;
use crate::storage::unix_timestamp_millis;

use super::context_bounds::ContextFields;
pub(super) use super::context_bounds::context_of;
use super::context_bounds::read_context;
use super::context_bounds::validate_context;

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
        mut context: KnowledgeContext,
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
        if context.source_sequence.is_none() && context.category == crate::KnowledgeCategory::Rule {
            context.source_sequence =
                Some(super::source_order::allocate_on(&mut transaction, &value.project_id).await?);
        }
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
        let next = super::source_order::allocate_on(&mut transaction, project_id).await?;
        transaction.commit().await?;
        Ok(next)
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

    /// The first journal row a direct action wrote, if it ran: a retried action finds what it
    /// did instead of doing it again.
    pub async fn change_for_action(
        &self,
        project_id: &str,
        action_id: &str,
    ) -> Result<Option<MemoryChange>, BlackboardStoreError> {
        sqlx::query_as::<_, StoredChange>(
            "SELECT * FROM memory_changes WHERE project_id = ? AND action_id = ?
             ORDER BY sequence LIMIT 1",
        )
        .bind(project_id)
        .bind(action_id)
        .fetch_optional(&self.pool)
        .await?
        .map(StoredChange::into_change)
        .transpose()
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

    /// A stored capture group with its members in order, so a receipt can be replayed.
    pub async fn capture_group(
        &self,
        project_id: &str,
        group_id: &str,
    ) -> Result<Option<CaptureGroup>, BlackboardStoreError> {
        let Some(row) = sqlx::query_as::<_, StoredGroup>(
            "SELECT * FROM capture_groups WHERE project_id = ? AND group_id = ?",
        )
        .bind(project_id)
        .bind(group_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let members = sqlx::query_as::<_, StoredMember>(
            "SELECT * FROM capture_group_members
             WHERE project_id = ? AND group_id = ? ORDER BY ordinal",
        )
        .bind(project_id)
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(StoredMember::into_member)
        .collect::<Result<Vec<_>, _>>()?;
        let count =
            |value: i64| u32::try_from(value).map_err(|_| BlackboardStoreError::RevisionOverflow);
        Ok(Some(CaptureGroup {
            project_id: row.project_id,
            group_id: row.group_id,
            thread_id: row.thread_id,
            turn_id: row.turn_id,
            kind: row.kind,
            declared_count: row.declared_count.map(count).transpose()?,
            recognized: count(row.recognized)?,
            saved: count(row.saved)?,
            already_present: count(row.already_present)?,
            pending: count(row.pending)?,
            omitted: count(row.omitted)?,
            failed: count(row.failed)?,
            members,
        }))
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
    validate_context(connection, context).await?;
    if project_id.len() > 512 || id.as_str().len() > 512 {
        return Err(BlackboardStoreError::UnsupportedContext);
    }
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
    if let Some(context) = read_context(
        &mut *connection,
        project_id,
        from.as_str(),
        ContextFields::Successor,
    )
    .await?
    {
        // The successor's words are new: what the payload said about the old words (who
        // spoke them) no longer holds, so it is not carried.
        let context = KnowledgeContext {
            payload: None,
            ..context
        };
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
    // An action identity binds one journaled change: a second writer of the same user action
    // (a concurrent retry) fails inside its own transaction, so it commits nothing.
    if let Some(action_id) = &change.action_id {
        let recorded = sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM memory_changes WHERE project_id = ? AND action_id = ?)",
        )
        .bind(project_id)
        .bind(action_id)
        .fetch_one(&mut *connection)
        .await?;
        if recorded != 0 {
            return Err(BlackboardStoreError::ActionAlreadyRecorded(
                action_id.clone(),
            ));
        }
    }
    let preview = bounded_preview(&change.preview);
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

pub(super) fn bounded_preview(text: &str) -> &str {
    if text.len() <= MAX_CHANGE_PREVIEW_BYTES {
        return text;
    }
    let mut end = MAX_CHANGE_PREVIEW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub(super) fn parse<T: std::str::FromStr<Err = String>>(
    value: &str,
) -> Result<T, BlackboardStoreError> {
    value
        .parse()
        .map_err(|error: String| BlackboardStoreError::InvalidStoredKnowledge(error))
}

fn unsigned(value: i64) -> Result<u64, BlackboardStoreError> {
    u64::try_from(value).map_err(|_| BlackboardStoreError::RevisionOverflow)
}

#[derive(FromRow)]
struct StoredGroup {
    project_id: String,
    group_id: String,
    thread_id: Option<String>,
    turn_id: Option<String>,
    kind: String,
    declared_count: Option<i64>,
    recognized: i64,
    saved: i64,
    already_present: i64,
    pending: i64,
    omitted: i64,
    failed: i64,
}

#[derive(FromRow)]
struct StoredMember {
    ordinal: i64,
    entry_id: Option<String>,
    revision: Option<i64>,
    outcome: String,
    preview: String,
    reason: Option<String>,
}

impl StoredMember {
    fn into_member(self) -> Result<CaptureGroupMember, BlackboardStoreError> {
        Ok(CaptureGroupMember {
            ordinal: u32::try_from(self.ordinal)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
            entry_id: self.entry_id,
            revision: self.revision.map(unsigned).transpose()?,
            outcome: parse(&self.outcome)?,
            preview: self.preview,
            reason: self.reason,
        })
    }
}

#[derive(FromRow)]
struct StoredChange {
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
    fn into_change(self) -> Result<MemoryChange, BlackboardStoreError> {
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
pub(super) mod tests;
