//! One writer transaction owns explicit action revisions, source ordering, outcomes and
//! journal. Callers prepare exact words; they emit receipts only after this method returns.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::ChangeOperation;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::MemberOutcome;
use crate::NewBlackboardEntry;
use crate::RootPromotion;
use sqlx::SqliteConnection;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::insert_new_entry;
use super::knowledge::append_change;
use super::knowledge::context_of;
use super::knowledge::write_context;
use super::load_entry;
use super::unix_timestamp_millis;
use super::write_revision;

/// One exact unit, with bounded candidate generations resolved under the writer lock.
#[derive(Clone, Debug)]
pub struct CaptureEntryWrite {
    pub candidates: Vec<BlackboardEntryId>,
    pub value: NewBlackboardEntry,
    pub context: KnowledgeContext,
    pub change: ChangeRecord,
}

/// All entries owned by one explicit action commit.
pub struct CaptureWrite {
    pub project_id: String,
    pub units: Vec<CaptureEntryWrite>,
}

/// Committed state from which the caller may report a receipt.
pub struct CaptureWriteResult {
    pub entries: Vec<(BlackboardEntry, MemberOutcome)>,
}

impl BlackboardStore {
    /// Atomically resolves and writes every unit. Any storage error rolls back the entire
    /// action, including its source position. A duplicate action fails and rolls
    /// back; its caller must compare the full recorded request before replaying it.
    pub async fn write_capture(
        &self,
        capture: CaptureWrite,
    ) -> Result<CaptureWriteResult, BlackboardStoreError> {
        if capture.units.is_empty() || capture.units.len() > 1024 {
            return Err(BlackboardStoreError::InvalidStoredKnowledge(
                "a capture must contain 1-1024 recognized units".to_string(),
            ));
        }
        let project_id = &capture.project_id;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        // Arbitrate every direct action under the writer lock before resolving entries.
        // This also makes a mismatched action retry a truthful action conflict rather than
        // an accidental entry-ID collision. Dropping this transaction rolls it back.
        for unit in &capture.units {
            if let Some(action_id) = &unit.change.action_id {
                let recorded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memory_changes WHERE project_id = ? AND action_id = ?)")
                    .bind(project_id).bind(action_id).fetch_one(&mut *transaction).await?;
                if recorded {
                    return Err(BlackboardStoreError::ActionAlreadyRecorded(
                        action_id.clone(),
                    ));
                }
            }
        }
        let now = unix_timestamp_millis()?;
        let sequence = super::source_order::allocate_on(&mut transaction, project_id).await?;
        let mut entries = Vec::new();
        for (ordinal, mut unit) in capture.units.into_iter().enumerate() {
            let ordinal =
                u32::try_from(ordinal).map_err(|_| BlackboardStoreError::CountOverflow)?;
            if unit.value.project_id != *project_id {
                return Err(BlackboardStoreError::InvalidStoredKnowledge(
                    "capture project mismatch".to_string(),
                ));
            }
            unit.context.source_sequence.get_or_insert(sequence);
            unit.context.unit_ordinal.get_or_insert(ordinal);
            let action_id = unit.change.action_id.clone();
            let fingerprint = unit.change.group_id.clone();
            if let Some((entry, outcome)) = write_unit(&mut transaction, unit, now).await? {
                if let Some(action_id) = action_id {
                    sqlx::query("INSERT INTO capture_action_outcomes (project_id, action_id, request_fingerprint, entry_id, revision, outcome) VALUES (?, ?, ?, ?, ?, ?)")
                        .bind(project_id).bind(action_id).bind(fingerprint).bind(entry.id.as_str())
                        .bind(i64::try_from(entry.revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?)
                        .bind(outcome.as_str()).execute(&mut *transaction).await?;
                }
                entries.push((entry, outcome));
            }
        }
        transaction.commit().await?;
        Ok(CaptureWriteResult { entries })
    }
}

async fn write_unit(
    connection: &mut SqliteConnection,
    unit: CaptureEntryWrite,
    now: i64,
) -> Result<Option<(BlackboardEntry, MemberOutcome)>, BlackboardStoreError> {
    unit.value.validate()?;
    if unit.candidates.is_empty() || unit.candidates.len() > 64 {
        return Err(BlackboardStoreError::InvalidStoredKnowledge(
            "capture identity bound".to_string(),
        ));
    }
    for id in &unit.candidates {
        let existing = load_entry(connection, &unit.value.project_id, id).await?;
        if let Some(mut entry) = existing {
            if entry.state != BlackboardEntryState::Active {
                continue;
            }
            let context = context_of(connection, &unit.value.project_id, id.as_str()).await?;
            if context.as_ref().is_some_and(|context| {
                context.category != unit.context.category
                    || context.authority != unit.context.authority
                    || context.scope_id != unit.context.scope_id
            }) || entry.value.kind != unit.value.kind
                || entry.value.content.split_whitespace().collect::<Vec<_>>()
                    != unit.value.content.split_whitespace().collect::<Vec<_>>()
            {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            let mut change = unit.change.clone();
            let outcome = if entry.value.root_promotion != RootPromotion::Promoted
                && unit.value.root_promotion == RootPromotion::Promoted
            {
                let revision = i64::try_from(entry.revision + 1)
                    .map_err(|_| BlackboardStoreError::RevisionOverflow)?;
                entry.value.root_promotion = RootPromotion::Promoted;
                sqlx::query(
                    "UPDATE blackboard_entries SET revision = ?, updated_at_ms = ? WHERE id = ?",
                )
                .bind(revision)
                .bind(now)
                .bind(id.as_str())
                .execute(&mut *connection)
                .await?;
                write_revision(
                    connection,
                    id,
                    revision,
                    &entry.value,
                    BlackboardEntryState::Active,
                    /*superseded_by*/ None,
                    now,
                )
                .await?;
                if let Some(context) = context {
                    write_context(connection, &unit.value.project_id, id, revision, &context)
                        .await?;
                }
                change.operation = ChangeOperation::Promoted;
                MemberOutcome::Saved
            } else {
                MemberOutcome::AlreadyPresent
            };
            let entry = load_entry(connection, &unit.value.project_id, id)
                .await?
                .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
            append_change(
                connection,
                &unit.value.project_id,
                Some((
                    id,
                    i64::try_from(entry.revision)
                        .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
                )),
                &change,
                now,
            )
            .await?;
            return Ok(Some((entry, outcome)));
        }
        insert_new_entry(connection, id, &unit.value, now).await?;
        write_context(connection, &unit.value.project_id, id, 1, &unit.context).await?;
        append_change(
            connection,
            &unit.value.project_id,
            Some((id, 1)),
            &unit.change,
            now,
        )
        .await?;
        let outcome = if unit.value.root_promotion == RootPromotion::Promoted {
            MemberOutcome::Saved
        } else {
            MemberOutcome::Pending
        };
        let entry = load_entry(connection, &unit.value.project_id, id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
        return Ok(Some((entry, outcome)));
    }
    Ok(None)
}
