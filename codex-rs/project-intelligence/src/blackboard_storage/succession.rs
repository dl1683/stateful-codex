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
use super::writer_policy::ModelOperation;
use super::writer_policy::WriterActor;
use super::writer_policy::check_model_target;

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
        let actor = if change.is_some_and(|change| change.origin == crate::ChangeOrigin::ModelTool)
        {
            WriterActor::Model
        } else {
            WriterActor::Host
        };
        self.create_successor_as(id, value, replaced, change, actor)
            .await
    }

    /// Creates or replays a model succession with authority checks inside the PI transaction.
    pub async fn create_successor_from_model(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
        replaced: Vec<SupersededEntry>,
    ) -> Result<Succession, BlackboardStoreError> {
        self.create_successor_as(
            id,
            value,
            replaced,
            /*change*/ None,
            WriterActor::Model,
        )
        .await
    }

    async fn create_successor_as(
        &self,
        id: BlackboardEntryId,
        mut value: NewBlackboardEntry,
        replaced: Vec<SupersededEntry>,
        change: Option<&crate::ChangeRecord>,
        actor: WriterActor,
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
        // Check before either first-write or committed-successor replay can return success.
        if matches!(actor, WriterActor::Model) {
            for target in &replaced {
                let current = load_entry(&mut transaction, &value.project_id, &target.id)
                    .await?
                    .ok_or_else(|| BlackboardStoreError::EntryNotFound(target.id.to_string()))?;
                check_model_target(&mut transaction, &current, ModelOperation::Retirement).await?;
            }
        }
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
        if matches!(actor, WriterActor::Model) {
            let context = super::context_bounds::read_context(
                &mut transaction,
                &value.project_id,
                replaced[0].id.as_str(),
                super::context_bounds::ContextFields::Identity,
            )
            .await?;
            super::identity::check_activation(&mut transaction, &value, context.as_ref()).await?;
        }
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

/// Select identifiers before loading any entry text or relations.
async fn predecessor_ids(
    connection: &mut sqlx::SqliteConnection,
    project_id: &str,
    successor_id: &BlackboardEntryId,
    limit: u32,
) -> Result<Vec<String>, BlackboardStoreError> {
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT entry.id
         FROM blackboard_entries AS entry
         JOIN blackboard_entry_revisions AS revision
           ON revision.entry_id = entry.id AND revision.revision = entry.revision
         WHERE entry.project_id = ? AND revision.state = 'superseded'
           AND revision.superseded_by = ?
         ORDER BY entry.id LIMIT ?",
    )
    .bind(project_id)
    .bind(successor_id.as_str())
    .bind(i64::from(limit))
    .fetch_all(&mut *connection)
    .await?)
}

/// Replay must compare the complete set, or refuse before loading full entries.
async fn load_superseded_by(
    connection: &mut sqlx::SqliteConnection,
    project_id: &str,
    successor_id: &BlackboardEntryId,
) -> Result<Vec<BlackboardEntry>, BlackboardStoreError> {
    let ids = predecessor_ids(
        connection,
        project_id,
        successor_id,
        (MAX_SUPERSEDED_ENTRIES + 1) as u32,
    )
    .await?;
    if ids.len() > MAX_SUPERSEDED_ENTRIES {
        return Err(BlackboardStoreError::EntryIdentityConflict(
            successor_id.to_string(),
        ));
    }
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
    /// The complete replacement set for replay. Refuses sets exceeding the succession cap.
    pub async fn superseded_by(
        &self,
        project_id: &str,
        successor_id: &BlackboardEntryId,
    ) -> Result<Vec<BlackboardEntry>, BlackboardStoreError> {
        let mut transaction = self.pool.begin().await?;
        let entries = load_superseded_by(&mut transaction, project_id, successor_id).await?;
        transaction.commit().await?;
        Ok(entries)
    }

    /// A bounded prefix of predecessors ordered by ID, and whether more exist.
    /// Selects at most `limit` IDs and loads only those entries, in one snapshot.
    /// The effective limit is capped at 200, including for historical stores.
    pub async fn predecessor_page(
        &self,
        project_id: &str,
        successor_id: &BlackboardEntryId,
        limit: u32,
    ) -> Result<(Vec<BlackboardEntry>, bool), BlackboardStoreError> {
        let limit = limit.min(200);
        let mut transaction = self.pool.begin().await?;
        let ids = predecessor_ids(&mut transaction, project_id, successor_id, limit).await?;
        // Probe for overflow without selecting another identifier or loading its entry.
        let more = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM blackboard_entries AS entry
                JOIN blackboard_entry_revisions AS revision
                  ON revision.entry_id = entry.id AND revision.revision = entry.revision
                WHERE entry.project_id = ? AND revision.state = 'superseded'
                  AND revision.superseded_by = ?
                LIMIT 1 OFFSET ?
             )",
        )
        .bind(project_id)
        .bind(successor_id.as_str())
        .bind(i64::from(limit))
        .fetch_one(&mut *transaction)
        .await?;
        let mut entries = Vec::with_capacity(ids.len());
        for id in ids {
            let id = BlackboardEntryId::parse(id)?;
            if let Some(entry) = load_entry(&mut transaction, project_id, &id).await? {
                entries.push(entry);
            }
        }
        transaction.commit().await?;
        Ok((entries, more))
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
        let mut transaction = self.pool.begin().await?;
        let rows = newest_predecessor_ids(&mut transaction, project_id, successor_ids).await?;
        let mut predecessors = Vec::new();
        for (successor_id, predecessor_id) in rows {
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

async fn newest_predecessor_ids(
    connection: &mut sqlx::SqliteConnection,
    project_id: &str,
    successor_ids: &[BlackboardEntryId],
) -> Result<Vec<(String, String)>, BlackboardStoreError> {
    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new("WITH requested(id) AS (VALUES ");
    let mut separated = builder.separated(", ");
    for id in successor_ids {
        separated
            .push("(")
            .push_bind_unseparated(id.as_str())
            .push_unseparated(")");
    }
    builder.push(
        ") SELECT DISTINCT requested.id, (
             SELECT entry.id FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ",
    );
    builder.push_bind(project_id);
    builder.push(
        " AND revision.state = 'superseded' AND revision.superseded_by = requested.id
          ORDER BY entry.updated_at_ms DESC, entry.id LIMIT 1
         ) AS predecessor_id FROM requested
         WHERE predecessor_id IS NOT NULL ORDER BY requested.id",
    );
    Ok(builder
        .build_query_as::<(String, String)>()
        .fetch_all(connection)
        .await?)
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
            sqlx::AssertSqlSafe(format!("SELECT entry.id FROM blackboard_entries AS entry JOIN blackboard_entry_revisions AS revision ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND entry.updated_at_ms >= ? {}
             ORDER BY entry.updated_at_ms DESC, entry.id LIMIT ?", super::identity::ENTRY_SOURCE_ELIGIBILITY)),
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
