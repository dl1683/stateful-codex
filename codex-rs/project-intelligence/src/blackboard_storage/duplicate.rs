//! Agent knowledge writes that never store the same record twice.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::NewBlackboardEntry;
use crate::storage::unix_timestamp_millis;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::CreateOutcome;
use super::insert_new_entry;
use super::knowledge::append_change;
use super::knowledge::write_context;
use super::load_entry;
use super::load_entry_by_id;

impl BlackboardStore {
    /// Creates an agent-recorded entry with its optional context and its journal row in one
    /// writer transaction. A replay of `id` (same project, kind, wording, node, structured
    /// value, verification, promotion, evidence and premises, from any call) is already
    /// present and writes nothing. Only an already-bound key is acknowledged this way: the
    /// same wording under a new key is a new record, because acknowledging it without a
    /// durable key binding would let that key later insert a different body.
    pub async fn create_agent_entry(
        &self,
        id: BlackboardEntryId,
        value: NewBlackboardEntry,
        context: Option<KnowledgeContext>,
        change: ChangeRecord,
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
        let now = unix_timestamp_millis()?;
        insert_new_entry(&mut transaction, &id, &value, now).await?;
        if let Some(context) = &context {
            write_context(&mut transaction, &value.project_id, &id, 1, context).await?;
        }
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
}

fn same_record(existing: &NewBlackboardEntry, new: &NewBlackboardEntry) -> bool {
    existing.project_id == new.project_id
        && existing.node_id == new.node_id
        && existing.kind == new.kind
        && existing.content == new.content
        && existing.structured_value == new.structured_value
        && existing.verification == new.verification
        && existing.root_promotion == new.root_promotion
        && existing.evidence == new.evidence
        && existing.premises == new.premises
        && existing.provenance.kind == new.provenance.kind
}
