//! Paged listing of a project's current knowledge for the user to review, in a fixed order:
//! rules first (the user's own before others), then decisions, then everything else.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;

/// One page of active entries, read in one snapshot with the memory revision it reflects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewPage {
    pub entries: Vec<BlackboardEntry>,
    pub more: bool,
    pub revision: u64,
}

impl BlackboardStore {
    /// Active entries of the project starting at `offset` in review order, at most `limit`,
    /// whether more follow, and the revision they reflect, all from one snapshot. The user's
    /// rules keep the order they were stored in (the order the user stated them); within the
    /// other groups, the most recently changed come first. With `expected_revision`, a page of a
    /// later revision is not read and `None` is returned.
    pub async fn active_review_page(
        &self,
        project_id: &str,
        offset: u32,
        limit: u32,
        expected_revision: Option<u64>,
    ) -> Result<Option<ReviewPage>, BlackboardStoreError> {
        let mut transaction = self.pool.begin().await?;
        let revision = sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM project_intelligence_revisions WHERE project_id = ?",
        )
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or_default();
        let revision =
            u64::try_from(revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        if expected_revision.is_some_and(|expected| expected != revision) {
            return Ok(None);
        }
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT entry.id
             FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE entry.project_id = ? AND revision.state = 'active'
             ORDER BY CASE
                 WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user' THEN 0
                 WHEN revision.kind = 'instruction' THEN 1
                 WHEN revision.kind = 'decision' THEN 2
                 ELSE 3 END,
               CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                   THEN (SELECT context.source_sequence FROM knowledge_context AS context
                         WHERE context.entry_id = entry.id
                         ORDER BY context.revision DESC LIMIT 1) END,
               CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                   THEN (SELECT context.unit_ordinal FROM knowledge_context AS context
                         WHERE context.entry_id = entry.id
                         ORDER BY context.revision DESC LIMIT 1) END,
               CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                   THEN entry.created_at_ms END,
               CASE WHEN revision.kind = 'instruction' AND revision.provenance_kind = 'user'
                   THEN entry.rowid END,
               entry.updated_at_ms DESC, entry.id
             LIMIT ? OFFSET ?",
        )
        .bind(project_id)
        .bind(i64::from(limit) + 1)
        .bind(i64::from(offset))
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
        Ok(Some(ReviewPage {
            entries,
            more,
            revision,
        }))
    }
}

/// Bounded context metadata used by public memory review, excluding historical text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewContext {
    pub authority: crate::KnowledgeAuthority,
    pub has_recorded_scope: bool,
}

impl BlackboardStore {
    /// Reads only authority and scope presence for the newest applicable context revision.
    pub async fn review_context(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
    ) -> Result<Option<ReviewContext>, BlackboardStoreError> {
        let row = sqlx::query_as::<_, (String, bool)>(
            "SELECT CASE WHEN octet_length(context.authority) > 32 THEN 'legacy_unknown'
                 WHEN context.authority IN ('human_direct','assistant_reported',
                 'reported_third_party','host_observed','legacy_unknown')
                 THEN context.authority ELSE 'legacy_unknown' END, context.scope_id IS NOT NULL
             FROM knowledge_context AS context
             JOIN blackboard_entries AS entry ON entry.id = context.entry_id
             WHERE context.project_id = ? AND context.entry_id = ?
               AND context.revision <= entry.revision
             ORDER BY context.revision DESC LIMIT 1",
        )
        .bind(project_id)
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(authority, has_recorded_scope)| {
            Ok(ReviewContext {
                authority: super::knowledge::parse(&authority)?,
                has_recorded_scope,
            })
        })
        .transpose()
    }
}
