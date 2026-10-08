//! Source-linked group transaction foundation. No automatic admission producer is exposed.
use super::BlackboardStore;
use super::BlackboardStoreError;
use crate::BlackboardEntryState;
use crate::CaptureEntryWrite;
use crate::CaptureGroup;
use crate::CaptureGroupMember;
use crate::ChangeOperation;
use crate::ChangeOrigin;
use crate::ChangeRecord;
use crate::KnowledgeCategory;
use crate::MemberOutcome;
use crate::SourceSeal;
use crate::SourceSpan;

#[cfg(test)]
#[path = "source_group_tests.rs"]
mod tests;

/// Each member retains its separately labelled original source ranges.
pub struct SourceCaptureMember {
    pub write: CaptureEntryWrite,
    pub seal: SourceSeal,
    pub spans: Vec<SourceSpan>,
}

/// One host action binds the whole group, not each member's journal row.
pub struct SourceCaptureGroup {
    pub project_id: String,
    pub action_id: String,
    pub group_id: String,
    pub members: Vec<SourceCaptureMember>,
}

impl BlackboardStore {
    /// Atomic entry/context/alias/source/member/action/journal commit; observation predates it.
    /// Retry returns committed members, but refuses if any member is no longer current.
    pub async fn write_source_group(
        &self,
        admission: &codex_state::ThreadProjectAdmission,
        group: SourceCaptureGroup,
    ) -> Result<CaptureGroup, BlackboardStoreError> {
        if group.members.is_empty()
            || group.members.len() > 24
            || group.action_id.is_empty()
            || group.action_id.len() > 128
            || group.group_id.is_empty()
            || group.group_id.len() > 512
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        if group.project_id != admission.project_id()
            || group.members.iter().any(|member| {
                member.seal.observation.authoritative_thread_id != admission.thread_id()
                    || member.seal.observation.binding_generation != admission.binding_generation()
            })
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let requests = group.members.iter().map(|member| {
            let context = &member.write.context;
            serde_json::json!({ "candidates": member.write.candidates, "value": member.write.value,
                "context": [context.category.as_str(), context.authority.as_str(), context.validity.as_str()],
                "scope": context.scope_id, "end": context.end_condition, "payload": context.payload,
                "sequence": context.source_sequence, "ordinal": context.unit_ordinal, "contextGroup": context.group_id,
                "origin": member.write.change.origin.as_str(), "operation": member.write.change.operation.as_str(),
                "category": member.write.change.category.as_str(), "preview": member.write.change.preview,
                "action": member.write.change.action_id, "thread": member.write.change.thread_id,
                "turn": member.write.change.turn_id, "changeGroup": member.write.change.group_id,
                "seal": member.seal, "spans": member.spans })
        }).collect::<Vec<_>>();
        let request = serde_json::json!({ "project": group.project_id, "group": group.group_id, "members": requests }).to_string();
        if request.len() > 32768 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let fingerprint = super::source::digest(&request);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let done: Option<(String, String)> = sqlx::query_as("SELECT group_id, request_fingerprint FROM capture_group_actions WHERE project_id = ? AND action_id = ?")
            .bind(&group.project_id).bind(&group.action_id).fetch_optional(&mut *tx).await?;
        if let Some((group_id, recorded)) = done {
            if group_id != group.group_id || recorded != fingerprint {
                return Err(BlackboardStoreError::ActionAlreadyRecorded(group.action_id));
            }
            let rows: Vec<(String, i64)> = sqlx::query_as("SELECT entry_id, revision FROM capture_group_members WHERE project_id = ? AND group_id = ? ORDER BY ordinal LIMIT 25")
                .bind(&group.project_id).bind(&group_id).fetch_all(&mut *tx).await?;
            if rows.len() != group.members.len() {
                return Err(BlackboardStoreError::InvalidSource);
            }
            for (id, revision) in rows {
                let id = crate::BlackboardEntryId::parse(id)?;
                let entry = super::load_entry(&mut tx, &group.project_id, &id)
                    .await?
                    .ok_or(BlackboardStoreError::InvalidSource)?;
                if entry.state != BlackboardEntryState::Active || entry.revision != revision as u64
                {
                    return Err(BlackboardStoreError::RetiredIdentity);
                }
                // Retained Step 2 policy also owns replay of model-origin members.
                if group
                    .members
                    .iter()
                    .any(|member| member.write.change.origin == ChangeOrigin::ModelTool)
                {
                    super::writer_policy::check_model_target(
                        &mut tx,
                        &entry,
                        super::writer_policy::ModelOperation::Mutation,
                    )
                    .await?;
                }
            }
            for member in &group.members {
                for span in &member.spans {
                    super::source::validate_span(
                        &mut tx,
                        &group.project_id,
                        &member.seal.exact_source_locator,
                        span,
                    )
                    .await?;
                }
            }
            let receipt =
                super::source_group_read::capture_group_on(&mut tx, &group.project_id, &group_id)
                    .await?
                    .ok_or(BlackboardStoreError::InvalidSource)?;
            tx.commit().await?;
            return Ok(receipt);
        }
        let direct_action: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memory_changes WHERE project_id = ? AND action_id = ?)",
        )
        .bind(&group.project_id)
        .bind(&group.action_id)
        .fetch_one(&mut *tx)
        .await?;
        if direct_action {
            return Err(BlackboardStoreError::ActionAlreadyRecorded(group.action_id));
        }
        let first = &group.members[0].seal.observation;
        let thread = first.authoritative_thread_id.clone();
        let turn = first.turn_id.clone();
        let mut envelope_ranges: std::collections::BTreeMap<String, Vec<(u32, u32)>> =
            std::collections::BTreeMap::new();
        for member in &group.members {
            let observation = &member.seal.observation;
            if observation.project_id != group.project_id
                || observation.authoritative_thread_id != thread
                || observation.turn_id != turn
                || !observation.complete_envelope
                || member.spans.is_empty()
                || member.spans.len() > 8
                || member.write.change.action_id.is_some()
            {
                return Err(BlackboardStoreError::InvalidSource);
            }
            let stored = super::source::seal_on(
                &mut tx,
                &group.project_id,
                &member.seal.exact_source_locator,
            )
            .await?
            .ok_or(BlackboardStoreError::InvalidSource)?;
            if stored != member.seal {
                return Err(BlackboardStoreError::InvalidSource);
            }
            let mut previous = 0;
            for span in &member.spans {
                if span.start_byte < previous
                    || span.start_byte >= span.end_byte
                    || span.end_byte > member.seal.original_utf8_length
                {
                    return Err(BlackboardStoreError::InvalidSource);
                }
                previous = span.end_byte;
                envelope_ranges
                    .entry(member.seal.exact_source_locator.clone())
                    .or_default()
                    .push((span.start_byte, span.end_byte));
                super::source::validate_span(
                    &mut tx,
                    &group.project_id,
                    &member.seal.exact_source_locator,
                    span,
                )
                .await?;
            }
        }
        let mut unique_bytes = 0u64;
        for ranges in envelope_ranges.values_mut() {
            ranges.sort_unstable();
            let mut end = 0;
            for &(start, next) in ranges.iter() {
                unique_bytes += u64::from(next.saturating_sub(end.max(start)));
                end = end.max(next);
            }
        }
        if unique_bytes > 16384 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let now = super::unix_timestamp_millis()?;
        let mut receipt = CaptureGroup {
            project_id: group.project_id.clone(),
            group_id: group.group_id.clone(),
            thread_id: Some(thread.clone()),
            turn_id: Some(turn.clone()),
            kind: "source-v1".to_string(),
            declared_count: Some(group.members.len() as u32),
            recognized: group.members.len() as u32,
            saved: 0,
            already_present: 0,
            pending: 0,
            omitted: 0,
            failed: 0,
            members: Vec::new(),
        };
        for (ordinal, mut member) in group.members.into_iter().enumerate() {
            member.write.context.source_sequence =
                Some(member.seal.immutable_first_observation_sequence);
            member.write.context.unit_ordinal = Some(ordinal as u32);
            member.write.context.group_id = Some(group.group_id.clone());
            member.write.change.group_id = Some(group.group_id.clone());
            let (entry, outcome) = super::capture_write::write_unit(&mut tx, member.write, now)
                .await?
                .ok_or(BlackboardStoreError::InvalidSource)?;
            for span in member.spans {
                sqlx::query("INSERT OR IGNORE INTO capture_entry_sources(entry_id, source_id, start_byte, end_byte, role) VALUES (?, ?, ?, ?, ?)")
                    .bind(entry.id.as_str()).bind(&member.seal.exact_source_locator).bind(i64::from(span.start_byte)).bind(i64::from(span.end_byte))
                    .bind(serde_json::to_string(&span.role).map_err(|_| BlackboardStoreError::InvalidSource)?).execute(&mut *tx).await?;
            }
            match outcome {
                MemberOutcome::Saved => receipt.saved += 1,
                MemberOutcome::AlreadyPresent => receipt.already_present += 1,
                MemberOutcome::Pending => receipt.pending += 1,
                MemberOutcome::Omitted => receipt.omitted += 1,
                MemberOutcome::Failed => receipt.failed += 1,
                MemberOutcome::NotRestored => return Err(BlackboardStoreError::RetiredIdentity),
            }
            receipt.members.push(CaptureGroupMember {
                ordinal: ordinal as u32,
                entry_id: Some(entry.id.to_string()),
                revision: Some(entry.revision),
                outcome,
                preview: super::knowledge::bounded_preview(&entry.value.content).to_string(),
                reason: None,
            });
        }
        let members_json = receipt.members.iter().map(|member| serde_json::json!({ "id": member.entry_id, "revision": member.revision, "outcome": member.outcome.as_str(), "preview": member.preview })).collect::<Vec<_>>();
        if serde_json::json!({"group": receipt.group_id, "project": receipt.project_id, "members": members_json}).to_string().len() > 16384 { return Err(BlackboardStoreError::InvalidSource); }
        sqlx::query("INSERT INTO capture_groups(project_id, group_id, thread_id, turn_id, kind, declared_count, recognized, saved, already_present, pending, omitted, failed, recorded_at_ms) VALUES (?, ?, ?, ?, 'source-v1', ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&group.project_id).bind(&group.group_id).bind(&thread).bind(&turn).bind(i64::from(receipt.recognized)).bind(i64::from(receipt.recognized))
            .bind(i64::from(receipt.saved)).bind(i64::from(receipt.already_present)).bind(i64::from(receipt.pending)).bind(i64::from(receipt.omitted)).bind(i64::from(receipt.failed)).bind(now).execute(&mut *tx).await?;
        for member in &receipt.members {
            sqlx::query("INSERT INTO capture_group_members(project_id, group_id, ordinal, entry_id, revision, outcome, preview, reason) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&group.project_id).bind(&group.group_id).bind(i64::from(member.ordinal)).bind(&member.entry_id).bind(member.revision.map(|revision| revision as i64))
                .bind(member.outcome.as_str()).bind(&member.preview).bind(&member.reason).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO capture_group_actions(project_id, action_id, group_id, request_fingerprint) VALUES (?, ?, ?, ?)")
            .bind(&group.project_id).bind(&group.action_id).bind(&group.group_id).bind(fingerprint).execute(&mut *tx).await?;
        super::knowledge::append_change(
            &mut tx,
            &group.project_id,
            /*entry*/ None,
            &ChangeRecord {
                operation: ChangeOperation::Saved,
                origin: ChangeOrigin::HostCapture,
                category: KnowledgeCategory::Note,
                action_id: Some(group.action_id),
                thread_id: Some(thread),
                turn_id: Some(turn),
                group_id: Some(group.group_id),
                preview: "source group committed".to_string(),
            },
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(receipt)
    }
}
