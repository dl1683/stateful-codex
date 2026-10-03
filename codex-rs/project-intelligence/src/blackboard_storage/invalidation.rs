//! Taking an entry out of current knowledge without deleting it. An invalidated entry stays
//! active and readable as history; a new revision records why it is no longer current (or
//! why it needs a check) and marks its verification stale.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardKind;
use crate::BlackboardVerification;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::KnowledgeValidity;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;

/// What an invalidation did.
#[derive(Clone, Debug, PartialEq)]
pub enum InvalidationOutcome {
    /// A new revision records the entry as no longer current.
    Invalidated(Box<BlackboardEntry>),
    /// The entry already had this validity (or was already obsolete); nothing changed.
    Unchanged,
}

impl BlackboardStore {
    /// Records `context` (whose validity must be `NeedsCheck` or `Obsolete`) for a new
    /// revision of the active entry `id` at `expected_revision`, journaling `change` in the
    /// same transaction. Its verification becomes stale (which also lets it keep evidence and
    /// premises that have since changed); content, links and promotion are kept, so the entry
    /// remains readable history. Repeating an invalidation,
    /// or asking for a check of an obsolete entry, changes nothing.
    pub async fn invalidate_entry(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
        expected_revision: u64,
        context: KnowledgeContext,
        change: &ChangeRecord,
    ) -> Result<InvalidationOutcome, BlackboardStoreError> {
        if !matches!(
            context.validity,
            KnowledgeValidity::NeedsCheck | KnowledgeValidity::Obsolete
        ) {
            return Err(BlackboardStoreError::InvalidStoredKnowledge(format!(
                "an invalidation records needs_check or obsolete, not {}",
                context.validity.as_str()
            )));
        }
        let current = self
            .get_entry(project_id, id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        if current.revision != expected_revision {
            return Err(BlackboardStoreError::RevisionConflict {
                expected: expected_revision,
                actual: current.revision,
            });
        }
        if current.state != BlackboardEntryState::Active {
            return Err(BlackboardStoreError::EntryNotActive(id.to_string()));
        }
        let validity = self
            .knowledge_context(project_id, id)
            .await?
            .map(|context| context.validity);
        if validity == Some(KnowledgeValidity::Obsolete) || validity == Some(context.validity) {
            return Ok(InvalidationOutcome::Unchanged);
        }
        let value = current.value;
        let update = BlackboardEntryUpdate {
            expected_revision,
            kind: value.kind,
            content: value.content,
            structured_value: value.structured_value,
            confidence: value.confidence,
            verification: BlackboardVerification::Stale,
            importance: value.importance,
            root_promotion: value.root_promotion,
            evidence: value.evidence,
            premises: value.premises,
            state: BlackboardEntryState::Active,
            superseded_by: None,
            provenance: value.provenance,
        };
        self.update_entry_with_context(project_id, id, update, Some(change), Some(&context))
            .await
            .map(|entry| InvalidationOutcome::Invalidated(Box::new(entry)))
    }
}

/// Which entries an invalidation may examine.
#[derive(Clone, Copy, Debug)]
pub struct InvalidationCandidates<'a> {
    /// Only these kinds.
    pub kinds: &'a [BlackboardKind],
    /// Only entries whose content contains this text.
    pub mentioning: &'a str,
    pub limit: u32,
}

impl BlackboardStore {
    /// Active, current entries that may still be invalidated: of the requested kinds,
    /// mentioning the text, not the user's own words, newest first. The flag says whether
    /// more matched than `limit`. Filtering happens before the limit, so unrelated entries
    /// cannot crowd out the relevant ones.
    pub async fn invalidation_candidates(
        &self,
        project_id: &str,
        candidates: InvalidationCandidates<'_>,
    ) -> Result<(Vec<BlackboardEntry>, bool), BlackboardStoreError> {
        if candidates.kinds.is_empty() {
            return Ok((Vec::new(), false));
        }
        let mut transaction = self.pool.begin().await?;
        let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT entry.id FROM blackboard_entries AS entry
             JOIN blackboard_entry_revisions AS revision
               ON revision.entry_id = entry.id AND revision.revision = entry.revision
             WHERE revision.state = 'active' AND revision.provenance_kind != 'user'
               AND COALESCE((SELECT context.validity FROM knowledge_context AS context
                   WHERE context.entry_id = entry.id AND context.revision <= entry.revision
                   ORDER BY context.revision DESC LIMIT 1), 'current')
                 = 'current'
               AND instr(revision.content, ",
        );
        builder.push_bind(candidates.mentioning);
        builder.push(") > 0 AND entry.project_id = ");
        builder.push_bind(project_id);
        builder.push(" AND revision.kind IN (");
        let mut kinds = builder.separated(", ");
        for kind in candidates.kinds {
            kinds.push_bind(super::kind_name(*kind));
        }
        builder.push(")");
        builder.push(" ORDER BY entry.rowid DESC LIMIT ");
        builder.push_bind(i64::from(candidates.limit) + 1);
        let mut ids = builder
            .build_query_scalar::<String>()
            .fetch_all(&mut *transaction)
            .await?;
        let more = ids.len() > candidates.limit as usize;
        ids.truncate(candidates.limit as usize);
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
}

#[cfg(test)]
#[path = "invalidation_tests.rs"]
mod tests;
