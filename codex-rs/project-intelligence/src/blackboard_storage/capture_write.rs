//! One writer transaction owns capture eligibility, revisions, scope binding, outcomes and
//! journal. Callers prepare exact words; they emit receipts only after this method returns.

use crate::BlackboardEntry;
use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::CaptureGroup;
use crate::CaptureGroupMember;
use crate::ChangeOperation;
use crate::ChangeRecord;
use crate::KnowledgeContext;
use crate::KnowledgeScope;
use crate::MemberOutcome;
use crate::NewBlackboardEntry;
use crate::RootPromotion;
use sqlx::SqliteConnection;

use super::BlackboardStore;
use super::BlackboardStoreError;
use super::insert_new_entry;
use super::knowledge::append_change;
use super::knowledge::bounded_preview;
use super::knowledge::context_of;
use super::knowledge::write_capture_group;
use super::knowledge::write_context;
use super::load_entry;
use super::unix_timestamp_millis;
use super::write_revision;

/// Whether creation is a fresh explicit action or a replayable conversation source.
#[derive(Clone, Debug)]
pub enum CaptureAuthority {
    DirectAction,
    /// Historical conversation sources never restore retired wording.
    Message,
}

/// One exact unit, with bounded candidate generations resolved under the writer lock.
#[derive(Clone, Debug)]
pub struct CaptureEntryWrite {
    pub candidates: Vec<BlackboardEntryId>,
    pub value: NewBlackboardEntry,
    pub context: KnowledgeContext,
    pub change: ChangeRecord,
    pub authority: CaptureAuthority,
}

/// A recognized unit either requests storage or already has an explicit omission outcome.
pub enum CaptureUnitWrite {
    Entry(Box<CaptureEntryWrite>),
    Outcome(CaptureGroupMember),
}

/// All state owned by one capture/action commit. A scope opens only with its source group.
pub struct CaptureWrite {
    pub project_id: String,
    pub group: Option<CaptureGroup>,
    pub scope: Option<KnowledgeScope>,
    pub units: Vec<CaptureUnitWrite>,
}

/// Committed state from which the caller may report a receipt.
pub struct CaptureWriteResult {
    pub group: Option<CaptureGroup>,
    pub entries: Vec<(BlackboardEntry, MemberOutcome)>,
}

impl BlackboardStore {
    /// Atomically resolves and writes every unit. Any storage error rolls back the entire
    /// group, including its scope and source position. A duplicate action fails and rolls
    /// back; its caller must compare the full recorded request before replaying it.
    pub async fn write_capture(
        &self,
        mut capture: CaptureWrite,
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
            if let CaptureUnitWrite::Entry(write) = unit
                && let Some(action_id) = &write.change.action_id
            {
                let recorded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memory_changes WHERE project_id = ? AND action_id = ?)")
                    .bind(project_id).bind(action_id).fetch_one(&mut *transaction).await?;
                if recorded {
                    return Err(BlackboardStoreError::ActionAlreadyRecorded(
                        action_id.clone(),
                    ));
                }
            }
        }
        if let Some(group) = &capture.group {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM capture_groups WHERE project_id = ? AND group_id = ?)",
            )
            .bind(project_id)
            .bind(&group.group_id)
            .fetch_one(&mut *transaction)
            .await?;
            if exists {
                transaction.rollback().await?;
                return Ok(CaptureWriteResult {
                    group: self.capture_group(project_id, &group.group_id).await?,
                    entries: Vec::new(),
                });
            }
        }
        let now = unix_timestamp_millis()?;
        if let Some(scope) = &capture.scope {
            let group = capture.group.as_ref().ok_or_else(|| {
                BlackboardStoreError::InvalidStoredKnowledge("scope requires a group".to_string())
            })?;
            sqlx::query(
                "INSERT INTO knowledge_scopes
                 (project_id, scope_id, kind, title, state, end_condition, opened_source,
                  ended_source, created_at_ms, updated_at_ms)
                 VALUES (?, ?, ?, ?, 'open', ?, ?, NULL, ?, ?)
                 ON CONFLICT(project_id, scope_id) DO NOTHING",
            )
            .bind(project_id)
            .bind(&scope.scope_id)
            .bind(scope.kind.as_str())
            .bind(&scope.title)
            .bind(&scope.end_condition)
            .bind(&scope.opened_source)
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            if scope.opened_source
                == format!(
                    "user-message:{}/{}",
                    group.thread_id.as_deref().unwrap_or_default(),
                    group.turn_id.as_deref().unwrap_or_default()
                )
            {
                sqlx::query(
                "INSERT INTO knowledge_scope_bindings (project_id, thread_id, scope_id, bound_at_ms)
                 VALUES (?, ?, ?, ?) ON CONFLICT(project_id, thread_id)
                 DO UPDATE SET scope_id = excluded.scope_id, bound_at_ms = excluded.bound_at_ms",
            )
            .bind(project_id).bind(&group.thread_id).bind(&scope.scope_id).bind(now)
            .execute(&mut *transaction).await?;
            } else {
                let bound: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_scope_bindings WHERE project_id = ? AND thread_id = ? AND scope_id = ?)")
                    .bind(project_id).bind(&group.thread_id).bind(&scope.scope_id).fetch_one(&mut *transaction).await?;
                if !bound {
                    return Err(BlackboardStoreError::ConcurrentMutation);
                }
            }
        }
        let sequence = super::source_order::allocate_on(&mut transaction, project_id).await?;
        let mut entries = Vec::new();
        let mut members = Vec::new();
        for (ordinal, unit) in capture.units.into_iter().enumerate() {
            let ordinal =
                u32::try_from(ordinal).map_err(|_| BlackboardStoreError::CountOverflow)?;
            let member = match unit {
                CaptureUnitWrite::Outcome(mut member) => {
                    member.ordinal = ordinal;
                    member
                }
                CaptureUnitWrite::Entry(mut unit) => {
                    if unit.value.project_id != *project_id {
                        return Err(BlackboardStoreError::InvalidStoredKnowledge(
                            "capture project mismatch".to_string(),
                        ));
                    }
                    unit.context.source_sequence.get_or_insert(sequence);
                    unit.context.unit_ordinal.get_or_insert(ordinal);
                    let preview = bounded_preview(&unit.value.content).to_string();
                    let action_id = unit.change.action_id.clone();
                    let fingerprint = unit.change.group_id.clone();
                    let result = write_unit(&mut transaction, *unit, now).await?;
                    match result {
                        Some((entry, outcome)) => {
                            if let Some(action_id) = action_id {
                                sqlx::query("INSERT INTO capture_action_outcomes (project_id, action_id, request_fingerprint, entry_id, revision, outcome) VALUES (?, ?, ?, ?, ?, ?)")
                                    .bind(project_id).bind(action_id).bind(fingerprint).bind(entry.id.as_str())
                                    .bind(i64::try_from(entry.revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?)
                                    .bind(outcome.as_str()).execute(&mut *transaction).await?;
                            }
                            let member = CaptureGroupMember {
                                ordinal,
                                entry_id: Some(entry.id.to_string()),
                                revision: Some(entry.revision),
                                outcome,
                                preview: bounded_preview(&entry.value.content).to_string(),
                                reason: None,
                            };
                            entries.push((entry, outcome));
                            member
                        }
                        None => CaptureGroupMember {
                            ordinal,
                            entry_id: None,
                            revision: None,
                            outcome: MemberOutcome::NotRestored,
                            preview,
                            reason: Some(
                                "source predates retirement or generation limit reached"
                                    .to_string(),
                            ),
                        },
                    }
                }
            };
            members.push(member);
        }
        if let Some(group) = &mut capture.group {
            group.recognized =
                u32::try_from(members.len()).map_err(|_| BlackboardStoreError::CountOverflow)?;
            group.saved = 0;
            group.already_present = 0;
            group.pending = 0;
            group.omitted = 0;
            group.failed = 0;
            for member in &members {
                match member.outcome {
                    MemberOutcome::Saved => group.saved += 1,
                    MemberOutcome::AlreadyPresent => group.already_present += 1,
                    MemberOutcome::Pending => group.pending += 1,
                    MemberOutcome::Omitted => group.omitted += 1,
                    MemberOutcome::Failed | MemberOutcome::NotRestored => group.failed += 1,
                }
            }
            group.members = members;
            if !write_capture_group(&mut transaction, group, now).await? {
                return Err(BlackboardStoreError::ConcurrentMutation);
            }
            let not_restored = group
                .members
                .iter()
                .filter(|member| member.outcome == MemberOutcome::NotRestored)
                .count() as u32;
            append_change(&mut transaction, project_id, /*entry*/ None, &ChangeRecord {
                operation: if group.failed + group.omitted + group.pending > 0 { ChangeOperation::CaptureIncomplete } else { ChangeOperation::Saved },
                origin: crate::ChangeOrigin::HostCapture, category: crate::KnowledgeCategory::Rule,
                action_id: None, thread_id: group.thread_id.clone(), turn_id: group.turn_id.clone(),
                group_id: Some(group.group_id.clone()),
                preview: format!("{} saved, {} reused, {} pending, {} omitted, {} failed, {not_restored} not-restored; {} recognized", group.saved, group.already_present, group.pending, group.omitted, group.failed - not_restored, group.recognized),
            }, now).await?;
        }
        transaction.commit().await?;
        Ok(CaptureWriteResult {
            group: capture.group,
            entries,
        })
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
    // Recheck scope/binding under the same writer lock as the entry and acknowledgement.
    if let Some(scope_id) = &unit.context.scope_id {
        let bound: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_scope_bindings AS binding
             JOIN knowledge_scopes AS scope USING (project_id, scope_id)
             WHERE binding.project_id = ? AND binding.thread_id = ?
               AND binding.scope_id = ? AND scope.state = 'open')",
        )
        .bind(&unit.value.project_id)
        .bind(&unit.change.thread_id)
        .bind(scope_id)
        .fetch_one(&mut *connection)
        .await?;
        if !bound {
            return Err(BlackboardStoreError::ConcurrentMutation);
        }
    }
    for id in &unit.candidates {
        let existing = load_entry(connection, &unit.value.project_id, id).await?;
        if let Some(mut entry) = existing {
            if entry.state != BlackboardEntryState::Active {
                if matches!(unit.authority, CaptureAuthority::Message) {
                    return Ok(None);
                }
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
