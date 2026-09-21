use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::NewBlackboardEntry;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::load_entry;
use super::unix_timestamp_millis;
use super::validate_evidence;
use super::write_revision;

impl BlackboardStore {
    pub async fn update_entry(
        &self,
        project_id: &str,
        id: &BlackboardEntryId,
        update: BlackboardEntryUpdate,
    ) -> Result<BlackboardEntry, BlackboardStoreError> {
        update.validate(id)?;
        let expected_revision = i64::try_from(update.expected_revision)
            .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
        let next_revision = expected_revision
            .checked_add(1)
            .ok_or(BlackboardStoreError::RevisionOverflow)?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_entry(&mut transaction, project_id, id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        if current.revision != update.expected_revision {
            return Err(BlackboardStoreError::RevisionConflict {
                expected: update.expected_revision,
                actual: current.revision,
            });
        }
        if current.state != BlackboardEntryState::Active {
            return Err(BlackboardStoreError::EntryNotActive(id.to_string()));
        }
        if let Some(successor_id) = update.superseded_by.as_ref() {
            let successor = load_entry(&mut transaction, project_id, successor_id)
                .await?
                .ok_or_else(|| BlackboardStoreError::SuccessorNotFound(successor_id.to_string()))?;
            if successor.state != BlackboardEntryState::Active {
                return Err(BlackboardStoreError::SuccessorNotActive(
                    successor_id.to_string(),
                ));
            }
        }
        let value = NewBlackboardEntry {
            project_id: current.value.project_id,
            node_id: current.value.node_id,
            kind: update.kind,
            content: update.content,
            structured_value: update.structured_value,
            confidence: update.confidence,
            verification: update.verification,
            importance: update.importance,
            root_promotion: update.root_promotion,
            evidence: update.evidence,
            provenance: update.provenance,
        };
        value.validate()?;
        validate_evidence(&mut transaction, &value).await?;
        let now = unix_timestamp_millis()?;
        let rows_affected = sqlx::query(
            "UPDATE blackboard_entries
             SET revision = ?, updated_at_ms = ?
             WHERE project_id = ? AND id = ? AND revision = ?",
        )
        .bind(next_revision)
        .bind(now)
        .bind(project_id)
        .bind(id.as_str())
        .bind(expected_revision)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if rows_affected != 1 {
            return Err(BlackboardStoreError::ConcurrentMutation);
        }
        write_revision(
            &mut transaction,
            id,
            next_revision,
            &value,
            update.state,
            update.superseded_by.as_ref(),
            now,
        )
        .await?;
        let entry = load_entry(&mut transaction, project_id, id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        transaction.commit().await?;
        Ok(entry)
    }
}
