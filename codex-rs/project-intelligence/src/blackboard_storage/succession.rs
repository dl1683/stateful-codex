//! Atomic replacement of current knowledge: a successor entry and the supersession of the
//! entries it replaces commit together or not at all.

use std::collections::HashSet;

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::NewBlackboardEntry;
use crate::RootPromotion;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::insert_new_entry;
use super::load_entry;
use super::load_entry_by_id;
use super::unix_timestamp_millis;
use super::write_revision;

/// Most entries one successor may replace.
pub const MAX_SUPERSEDED_ENTRIES: usize = 4;

/// An entry to replace, at the revision the caller saw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupersededEntry {
    pub id: BlackboardEntryId,
    pub expected_revision: u64,
}

/// The stored successor and the replaced entries in their superseded revisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Succession {
    pub successor: BlackboardEntry,
    pub superseded: Vec<BlackboardEntry>,
}

impl BlackboardStore {
    /// Creates `id` and marks every entry in `replaced` superseded by it, in one transaction.
    /// The successor is promoted when any replaced entry was. A retry with the same successor
    /// whose replaced entries are already superseded by it returns the stored result.
    pub async fn create_successor(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
        replaced: Vec<SupersededEntry>,
    ) -> Result<Succession, BlackboardStoreError> {
        self.create_successor_recorded(id, value, replaced, /*change*/ None)
            .await
    }

    /// Like `create_successor`, journaling `change` for the successor in the same
    /// transaction. The successor takes the context (category, scope, position) of the first
    /// entry it replaces.
    pub async fn create_successor_recorded(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
        replaced: Vec<SupersededEntry>,
        change: Option<&crate::ChangeRecord>,
    ) -> Result<Succession, BlackboardStoreError> {
        self.create_successor_accounted(id, value, replaced, change, /*replaced_change*/ None)
            .await
    }

    /// Like `create_successor_recorded`, also journaling `replaced_change` (with each replaced
    /// entry's own text and category) for every entry the successor replaces, all in the same
    /// transaction. A retry of a committed succession journals nothing.
    pub async fn create_successor_accounted(
        &self,
        id: BlackboardEntryId,
        mut value: NewBlackboardEntry,
        replaced: Vec<SupersededEntry>,
        change: Option<&crate::ChangeRecord>,
        replaced_change: Option<&crate::ChangeRecord>,
    ) -> Result<Succession, BlackboardStoreError> {
        value.validate()?;
        let unique = replaced
            .iter()
            .map(|entry| &entry.id)
            .collect::<HashSet<_>>();
        if replaced.is_empty()
            || replaced.len() > MAX_SUPERSEDED_ENTRIES
            || unique.len() != replaced.len()
            || unique.contains(&id)
        {
            return Err(BlackboardStoreError::InvalidSuccession);
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_entry_by_id(&mut transaction, &id).await? {
            // A retry is accepted only when it is the same request against a successor that
            // is still current: same value (with the promotion it inherited), and exactly the
            // named entries superseded by it, each at the revision the request named.
            let superseded = load_superseded_by(&mut transaction, &value.project_id, &id).await?;
            let same_request = superseded.len() == replaced.len()
                && replaced.iter().all(|entry| {
                    superseded.iter().any(|current| {
                        current.id == entry.id
                            && current.revision == entry.expected_revision.saturating_add(1)
                    })
                });
            if !same_request {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            let mut expected = value.clone();
            if superseded
                .iter()
                .any(|entry| entry.value.root_promotion == RootPromotion::Promoted)
            {
                expected.root_promotion = RootPromotion::Promoted;
            }
            if existing.state != BlackboardEntryState::Active || existing.value != expected {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
            return Ok(Succession {
                successor: existing,
                superseded,
            });
        }
        let mut current_entries = Vec::with_capacity(replaced.len());
        for entry in &replaced {
            let current = load_entry(&mut transaction, &value.project_id, &entry.id)
                .await?
                .ok_or_else(|| BlackboardStoreError::EntryNotFound(entry.id.to_string()))?;
            if current.revision != entry.expected_revision {
                return Err(BlackboardStoreError::RevisionConflict {
                    expected: entry.expected_revision,
                    actual: current.revision,
                });
            }
            if current.state != BlackboardEntryState::Active {
                return Err(BlackboardStoreError::EntryNotActive(entry.id.to_string()));
            }
            current_entries.push(current);
        }
        if current_entries
            .iter()
            .any(|entry| entry.value.root_promotion == RootPromotion::Promoted)
        {
            value.root_promotion = RootPromotion::Promoted;
        }
        let now = unix_timestamp_millis()?;
        insert_new_entry(&mut transaction, &id, &value, now).await?;
        if let Some(first) = replaced.first() {
            super::knowledge::carry_context(
                &mut transaction,
                &value.project_id,
                &first.id,
                &id,
                /*revision*/ 1,
            )
            .await?;
        }
        if let Some(change) = change {
            super::knowledge::append_change(
                &mut transaction,
                &value.project_id,
                Some((&id, 1)),
                change,
                now,
            )
            .await?;
        }
        let mut superseded = Vec::with_capacity(current_entries.len());
        for current in current_entries {
            let expected_revision = i64::try_from(current.revision)
                .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
            let next_revision = expected_revision
                .checked_add(1)
                .ok_or(BlackboardStoreError::RevisionOverflow)?;
            let rows_affected = sqlx::query(
                "UPDATE blackboard_entries
                 SET revision = ?, updated_at_ms = ?
                 WHERE project_id = ? AND id = ? AND revision = ?",
            )
            .bind(next_revision)
            .bind(now)
            .bind(&value.project_id)
            .bind(current.id.as_str())
            .bind(expected_revision)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if rows_affected != 1 {
                return Err(BlackboardStoreError::ConcurrentMutation);
            }
            // The superseded revision keeps its meaning, evidence and authorship; who
            // replaced it is the successor's provenance.
            write_revision(
                &mut transaction,
                &current.id,
                next_revision,
                &current.value,
                BlackboardEntryState::Superseded,
                Some(&id),
                now,
            )
            .await?;
            if let Some(replaced_change) = replaced_change {
                let mut record = replaced_change.clone();
                record.preview = current.value.content.clone();
                // Each replaced entry keeps its own category (legacy when none was recorded).
                record.category = super::knowledge::context_of(
                    &mut transaction,
                    &value.project_id,
                    current.id.as_str(),
                )
                .await?
                .map_or(crate::KnowledgeCategory::Legacy, |context| context.category);
                super::knowledge::append_change(
                    &mut transaction,
                    &value.project_id,
                    Some((&current.id, next_revision)),
                    &record,
                    now,
                )
                .await?;
            }
            superseded.push(
                load_entry(&mut transaction, &value.project_id, &current.id)
                    .await?
                    .ok_or_else(|| BlackboardStoreError::EntryNotFound(current.id.to_string()))?,
            );
        }
        let successor = load_entry(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(Succession {
            successor,
            superseded,
        })
    }
}

#[cfg(test)]
#[path = "succession_tests.rs"]
mod tests;

/// The entries currently recorded as superseded by `successor_id`.
async fn load_superseded_by(
    connection: &mut sqlx::SqliteConnection,
    project_id: &str,
    successor_id: &BlackboardEntryId,
) -> Result<Vec<BlackboardEntry>, BlackboardStoreError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT entry.id
         FROM blackboard_entries AS entry
         JOIN blackboard_entry_revisions AS revision
           ON revision.entry_id = entry.id AND revision.revision = entry.revision
         WHERE entry.project_id = ? AND revision.state = 'superseded'
           AND revision.superseded_by = ?
         ORDER BY entry.id",
    )
    .bind(project_id)
    .bind(successor_id.as_str())
    .fetch_all(&mut *connection)
    .await?;
    let mut entries = Vec::with_capacity(ids.len());
    for id in ids {
        let id = BlackboardEntryId::parse(id)?;
        if let Some(entry) = load_entry(&mut *connection, project_id, &id).await? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

impl BlackboardStore {
    /// The entries `successor_id` replaced, as currently recorded.
    pub async fn superseded_by(
        &self,
        project_id: &str,
        successor_id: &BlackboardEntryId,
    ) -> Result<Vec<BlackboardEntry>, BlackboardStoreError> {
        let mut connection = self.pool.acquire().await?;
        load_superseded_by(&mut connection, project_id, successor_id).await
    }

    /// The newest entry each of `successor_ids` replaced, for showing what a current value
    /// replaced and since when. Entries that replaced nothing are absent.
    pub async fn newest_predecessors(
        &self,
        project_id: &str,
        successor_ids: &[BlackboardEntryId],
    ) -> Result<Vec<(BlackboardEntryId, BlackboardEntry)>, BlackboardStoreError> {
        if successor_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT revision.superseded_by, entry.id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE revision.state = 'superseded' AND entry.project_id = ",
        );
        builder.push_bind(project_id);
        builder.push(" AND revision.superseded_by IN (");
        let mut separated = builder.separated(", ");
        for successor_id in successor_ids {
            separated.push_bind(successor_id.as_str());
        }
        builder.push(") ORDER BY revision.superseded_by, entry.updated_at_ms DESC, entry.id");
        let mut transaction = self.pool.begin().await?;
        // One pass over the project's superseded entries, newest first per successor.
        let rows = builder
            .build_query_as::<(String, String)>()
            .fetch_all(&mut *transaction)
            .await?;
        let mut predecessors = Vec::new();
        let mut seen = HashSet::new();
        for (successor_id, predecessor_id) in rows {
            if !seen.insert(successor_id.clone()) {
                continue;
            }
            let predecessor_id = BlackboardEntryId::parse(predecessor_id)?;
            if let Some(predecessor) =
                load_entry(&mut transaction, project_id, &predecessor_id).await?
            {
                predecessors.push((BlackboardEntryId::parse(successor_id)?, predecessor));
            }
        }
        transaction.commit().await?;
        Ok(predecessors)
    }
}

impl BlackboardStore {
    /// Entries of the project recorded or changed at or after `since_ms`, in any state, newest
    /// first, at most `limit`; the flag says whether more exist.
    pub async fn changed_since(
        &self,
        project_id: &str,
        since_ms: i64,
        limit: u32,
    ) -> Result<(Vec<BlackboardEntry>, bool), BlackboardStoreError> {
        let mut transaction = self.pool.begin().await?;
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT id FROM blackboard_entries
             WHERE project_id = ? AND updated_at_ms >= ?
             ORDER BY updated_at_ms DESC, id LIMIT ?",
        )
        .bind(project_id)
        .bind(since_ms)
        .bind(i64::from(limit) + 1)
        .fetch_all(&mut *transaction)
        .await?;
        let more = ids.len() > limit as usize;
        let mut entries = Vec::with_capacity(ids.len().min(limit as usize));
        for id in ids.into_iter().take(limit as usize) {
            let id = BlackboardEntryId::parse(id)?;
            if let Some(entry) = load_entry(&mut transaction, project_id, &id).await? {
                entries.push(entry);
            }
        }
        transaction.commit().await?;
        Ok((entries, more))
    }
}
