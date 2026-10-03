//! Cheap reads of what project memory holds and what changed in it: a census of active
//! entries for the passive status line, journal totals for session and exit receipts, and
//! journal pages restricted to a set of threads.

use sqlx::FromRow;
use sqlx::QueryBuilder;
use sqlx::Sqlite;

use crate::BlackboardKind;
use crate::BlackboardProvenanceKind;
use crate::ChangeOperation;
use crate::KnowledgeCategory;
use crate::KnowledgeValidity;
use crate::MemoryChange;
use crate::RootPromotion;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::knowledge::MAX_CHANGES_PAGE;
use super::knowledge::StoredChange;
use super::parse_kind;
use super::parse_promotion;
use super::parse_provenance;

/// What classifies one active entry, without its content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CensusEntry {
    pub id: String,
    pub kind: BlackboardKind,
    pub provenance: BlackboardProvenanceKind,
    pub root_promotion: RootPromotion,
    /// The category recorded with its current revision; `None` for legacy entries.
    pub category: Option<KnowledgeCategory>,
    /// Whether it still describes the present; `None` for legacy entries.
    pub validity: Option<KnowledgeValidity>,
    pub updated_at_ms: i64,
}

/// How many journal rows of one operation and category a stretch of the journal holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeCount {
    pub operation: ChangeOperation,
    pub category: KnowledgeCategory,
    pub count: u32,
}

/// The newest journal row's sequence and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalHead {
    pub sequence: u64,
    pub created_at_ms: i64,
}

impl BlackboardStore {
    /// Every active entry of the project, classified but without content.
    pub async fn memory_census(
        &self,
        project_id: &str,
    ) -> Result<Vec<CensusEntry>, BlackboardStoreError> {
        let rows = sqlx::query_as::<_, StoredCensus>(
            "SELECT entry.id AS id, revision.kind AS kind,
                    revision.provenance_kind AS provenance_kind,
                    revision.root_promotion AS root_promotion,
                    (SELECT context.category FROM knowledge_context AS context
                     WHERE context.entry_id = entry.id AND context.revision <= entry.revision
                     ORDER BY context.revision DESC LIMIT 1) AS category,
                    (SELECT context.validity FROM knowledge_context AS context
                     WHERE context.entry_id = entry.id AND context.revision <= entry.revision
                     ORDER BY context.revision DESC LIMIT 1) AS validity,
                    entry.updated_at_ms AS updated_at_ms
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(CensusEntry {
                    id: row.id,
                    kind: parse_kind(&row.kind)?,
                    provenance: parse_provenance(&row.provenance_kind)?,
                    root_promotion: parse_promotion(&row.root_promotion)?,
                    category: row
                        .category
                        .map(|category| {
                            category
                                .parse()
                                .map_err(BlackboardStoreError::InvalidStoredKnowledge)
                        })
                        .transpose()?,
                    validity: row
                        .validity
                        .map(|validity| {
                            validity
                                .parse()
                                .map_err(BlackboardStoreError::InvalidStoredKnowledge)
                        })
                        .transpose()?,
                    updated_at_ms: row.updated_at_ms,
                })
            })
            .collect()
    }

    /// Journal rows after `after`, counted by operation and category, optionally only those
    /// recorded for `thread_ids`.
    pub async fn change_totals(
        &self,
        project_id: &str,
        after: u64,
        thread_ids: Option<&[String]>,
    ) -> Result<Vec<ChangeCount>, BlackboardStoreError> {
        let after = i64::try_from(after).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT operation, category, COUNT(*) FROM memory_changes WHERE project_id = ",
        );
        query
            .push_bind(project_id)
            .push(" AND sequence > ")
            .push_bind(after);
        push_thread_filter(&mut query, thread_ids);
        query.push(" GROUP BY operation, category ORDER BY operation, category");
        let rows = query
            .build_query_as::<(String, String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|(operation, category, count)| {
                Ok(ChangeCount {
                    operation: operation
                        .parse()
                        .map_err(BlackboardStoreError::InvalidStoredKnowledge)?,
                    category: category
                        .parse()
                        .map_err(BlackboardStoreError::InvalidStoredKnowledge)?,
                    count: u32::try_from(count).map_err(|_| BlackboardStoreError::CountOverflow)?,
                })
            })
            .collect()
    }

    /// Journal rows after `after`, oldest first, at most `limit`, optionally only those
    /// recorded for `thread_ids`.
    pub async fn memory_changes_for_threads(
        &self,
        project_id: &str,
        after: u64,
        thread_ids: Option<&[String]>,
        limit: u32,
    ) -> Result<Vec<MemoryChange>, BlackboardStoreError> {
        let after = i64::try_from(after).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        let limit = i64::from(limit.clamp(1, MAX_CHANGES_PAGE));
        let mut query =
            QueryBuilder::<Sqlite>::new("SELECT * FROM memory_changes WHERE project_id = ");
        query
            .push_bind(project_id)
            .push(" AND sequence > ")
            .push_bind(after);
        push_thread_filter(&mut query, thread_ids);
        query.push(" ORDER BY sequence LIMIT ").push_bind(limit);
        let rows = query
            .build_query_as::<StoredChange>()
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter().map(StoredChange::into_change).collect()
    }

    /// The newest journal sequence recorded at or before `at_ms` (0 when none was).
    pub async fn sequence_at(
        &self,
        project_id: &str,
        at_ms: i64,
    ) -> Result<u64, BlackboardStoreError> {
        let sequence = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(sequence) FROM memory_changes WHERE project_id = ? AND created_at_ms <= ?",
        )
        .bind(project_id)
        .bind(at_ms)
        .fetch_one(&self.pool)
        .await?
        .unwrap_or(0);
        u64::try_from(sequence).map_err(|_| BlackboardStoreError::RevisionOverflow)
    }

    /// The newest journal rows of one operation and category, newest first, at most `limit`.
    pub async fn recent_changes(
        &self,
        project_id: &str,
        operation: ChangeOperation,
        category: KnowledgeCategory,
        limit: u32,
    ) -> Result<Vec<MemoryChange>, BlackboardStoreError> {
        let rows = sqlx::query_as::<_, StoredChange>(
            "SELECT * FROM memory_changes
             WHERE project_id = ? AND operation = ? AND category = ?
             ORDER BY sequence DESC LIMIT ?",
        )
        .bind(project_id)
        .bind(operation.as_str())
        .bind(category.as_str())
        .bind(i64::from(limit.clamp(1, MAX_CHANGES_PAGE)))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(StoredChange::into_change).collect()
    }

    /// The newest journal row of the project, if any.
    pub async fn journal_head(
        &self,
        project_id: &str,
    ) -> Result<Option<JournalHead>, BlackboardStoreError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT sequence, created_at_ms FROM memory_changes
             WHERE project_id = ? ORDER BY sequence DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(sequence, created_at_ms)| {
            Ok(JournalHead {
                sequence: u64::try_from(sequence)
                    .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
                created_at_ms,
            })
        })
        .transpose()
    }
}

/// Restricts a journal query to rows recorded for `thread_ids` (none match an empty list).
fn push_thread_filter(query: &mut QueryBuilder<Sqlite>, thread_ids: Option<&[String]>) {
    let Some(thread_ids) = thread_ids else {
        return;
    };
    if thread_ids.is_empty() {
        query.push(" AND 0");
        return;
    }
    query.push(" AND thread_id IN (");
    let mut separated = query.separated(", ");
    for thread_id in thread_ids {
        separated.push_bind(thread_id.clone());
    }
    query.push(")");
}

#[derive(FromRow)]
struct StoredCensus {
    id: String,
    kind: String,
    provenance_kind: String,
    root_promotion: String,
    category: Option<String>,
    validity: Option<String>,
    updated_at_ms: i64,
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
