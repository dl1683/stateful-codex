//! Agent knowledge writes that never store the same record twice.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardProvenanceKind;
use crate::KnowledgeContext;
use crate::NewBlackboardEntry;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::CreateOutcome;
use super::insert_new_entry;
use super::kind_name;
use super::knowledge::write_context;
use super::load_entry;
use super::load_entry_by_id;

impl BlackboardStore {
    /// Creates an agent-recorded entry with its optional context in one writer transaction,
    /// unless the same record is already current knowledge: a replay of `id`, or an active
    /// agent entry with the same kind, wording, node, structured value, verification,
    /// promotion, evidence and premises. Confidence, importance and the recording call do
    /// not make a record new. Records that differ in any meaningful field are created.
    pub async fn create_agent_entry(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
        context: Option<KnowledgeContext>,
    ) -> Result<(BlackboardEntry, CreateOutcome), BlackboardStoreError> {
        value.validate()?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) = load_entry_by_id(&mut transaction, &id).await? {
            // A replay may come from another call; the recording call is not the record.
            if !same_record(&existing.value, &value)
                || existing.state != BlackboardEntryState::Active
                || existing.superseded_by.is_some()
            {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            transaction.commit().await?;
            return Ok((existing, CreateOutcome::AlreadyPresent));
        }
        if value.provenance.kind == BlackboardProvenanceKind::Agent {
            let candidates = sqlx::query_scalar::<_, String>(
                "SELECT entry.id
                 FROM blackboard_entries AS entry
                 JOIN blackboard_entry_revisions AS revision
                   ON revision.entry_id = entry.id AND revision.revision = entry.revision
                 WHERE entry.project_id = ? AND revision.state = 'active'
                   AND revision.provenance_kind = 'agent'
                   AND revision.kind = ? AND revision.content = ?
                 ORDER BY entry.created_at_ms, entry.id",
            )
            .bind(&value.project_id)
            .bind(kind_name(value.kind))
            .bind(&value.content)
            .fetch_all(&mut *transaction)
            .await?;
            for candidate in candidates {
                let candidate = BlackboardEntryId::parse(candidate)
                    .map_err(BlackboardStoreError::InvalidEntry)?;
                if let Some(existing) =
                    load_entry(&mut transaction, &value.project_id, &candidate).await?
                    && same_record(&existing.value, &value)
                {
                    transaction.commit().await?;
                    return Ok((existing, CreateOutcome::AlreadyPresent));
                }
            }
        }
        let now = unix_timestamp_millis()?;
        insert_new_entry(&mut transaction, &id, &value, now).await?;
        if let Some(context) = &context {
            write_context(&mut transaction, &value.project_id, &id, 1, context).await?;
        }
        let entry = load_entry(&mut transaction, &value.project_id, &id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok((entry, CreateOutcome::Created))
    }
}

fn same_record(existing: &NewBlackboardEntry, new: &NewBlackboardEntry) -> bool {
    existing.node_id == new.node_id
        && existing.kind == new.kind
        && existing.content == new.content
        && existing.structured_value == new.structured_value
        && existing.verification == new.verification
        && existing.root_promotion == new.root_promotion
        && existing.evidence == new.evidence
        && existing.premises == new.premises
        && existing.provenance.kind == new.provenance.kind
}
