//! Explicit user controls over committed captures: Apply (promotion of a retained source
//! proposal) and receipt Undo. Both run under the authoritative project admission and the PI
//! writer lock; neither is reachable from a model tool. Promotion keeps the proposal's entry
//! identity and original source and adds a revision; Undo touches only what its receipt made.
use super::BlackboardStore;
use super::BlackboardStoreError;
use super::source::digest;
use crate::*;
use serde_json::json;
use sqlx::SqliteConnection;

#[cfg(test)]
#[path = "promotion_tests.rs"]
mod tests;

/// What the user applies a proposal as. Applied words always take project scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromotionCategory {
    /// A standing project rule in the user's quoted words.
    Rule,
    /// The user's settled decision, with the reason quoted with it.
    Decision,
    /// An approach the user ruled out, with the reason quoted with it.
    RuledOut,
}

impl PromotionCategory {
    fn knowledge(self) -> (BlackboardKind, KnowledgeCategory) {
        match self {
            Self::Rule => (BlackboardKind::Instruction, KnowledgeCategory::Rule),
            Self::Decision => (BlackboardKind::Decision, KnowledgeCategory::Decision),
            Self::RuledOut => (
                BlackboardKind::RejectedApproach,
                KnowledgeCategory::RuledOut,
            ),
        }
    }
}

/// One explicit Apply act naming the proposal revision the user saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromotionRequest {
    pub entry_id: BlackboardEntryId,
    pub expected_revision: u64,
    pub category: PromotionCategory,
    pub action_id: String,
}

/// Largest sealed part an Apply can take whole; longer sources stay proposals.
const MAX_APPLIED_BYTES: u32 = 4096;

impl BlackboardStore {
    /// Applies a retained proposal as the user's own rule, decision or ruled-out approach.
    /// The applied words are the exact contiguous source range the proposal cited, never the
    /// model's interpretation. A retry of the same action returns the recorded receipt.
    pub async fn promote_proposal(
        &self,
        admission: &codex_state::ThreadProjectAdmission,
        request: PromotionRequest,
    ) -> Result<CaptureGroup, BlackboardStoreError> {
        let project = admission.project_id();
        if request.action_id.trim().is_empty()
            || request.action_id.len() > 128
            || request.entry_id.as_str().len() > 512
            || request.expected_revision == 0
            || request.expected_revision >= i64::MAX as u64
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let fingerprint = digest(
            &json!([
                project,
                "promote",
                request.entry_id.as_str(),
                request.expected_revision,
                format!("{:?}", request.category)
            ])
            .to_string(),
        );
        let group_id = format!(
            "promotion-{}",
            digest(&json!([project, request.action_id]).to_string())
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) =
            recorded_action(&mut tx, project, &request.action_id, &fingerprint).await?
        {
            // A recorded Apply is acknowledged only while what it applied is still current;
            // after a Forget, Undo or correction the retry refuses and restores nothing.
            for member in &receipt.members {
                let (Some(id), Some(revision)) = (&member.entry_id, member.revision) else {
                    continue;
                };
                let id = BlackboardEntryId::parse(id.clone())?;
                let current = super::load_entry(&mut tx, project, &id).await?;
                if !current.is_some_and(|current| {
                    current.revision == revision && current.state == BlackboardEntryState::Active
                }) {
                    return Err(BlackboardStoreError::EntryNotActive(format!(
                        "{id}@{revision}: this action applied it earlier, but it has since been forgotten, undone or corrected; nothing was restored"
                    )));
                }
            }
            tx.commit().await?;
            return Ok(receipt);
        }
        let entry = super::load_entry(&mut tx, project, &request.entry_id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(request.entry_id.to_string()))?;
        if entry.revision != request.expected_revision {
            return Err(BlackboardStoreError::RevisionConflict {
                expected: request.expected_revision,
                actual: entry.revision,
            });
        }
        if entry.state != BlackboardEntryState::Active {
            return Err(BlackboardStoreError::EntryNotActive(entry.id.to_string()));
        }
        let Applicable {
            previous,
            payload,
            proposal,
            seal,
            quoted,
            quoted_start,
            quoted_end,
        } = applicable(&mut tx, project, &entry, request.category).await?;
        let quoted = quoted.as_str();
        let (kind, category) = request.category.knowledge();
        let now = super::unix_timestamp_millis()?;
        let mut temporal: TemporalContext = payload
            .get("temporal")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| BlackboardStoreError::UnsupportedContext)?
            .ok_or(BlackboardStoreError::InvalidSource)?;
        // A live user turn's time is host-observed; a stated or attributed time is kept.
        if temporal.source_time == SourceTime::Unknown {
            temporal.source_time = SourceTime::HostObserved {
                unix_ms: seal.recorded_at_ms,
                precision: TimePrecision::Second,
            };
        }
        temporal.recorded_at_ms = now;
        temporal.validate()?;
        let quotation = json!({"sourceId": seal.exact_source_locator, "digest": seal.digest,
            "sourceRevision": seal.observation.source_revision, "partIndex": seal.observation.part_index,
            "startByte": quoted_start, "endByte": quoted_end});
        let mut meaning = json!({"promotion": {"version": 1, "by": "user", "actionId": request.action_id,
            "threadId": admission.thread_id(), "promotedAtMs": now, "fromRevision": entry.revision,
            "fromGroupId": previous.group_id, "quoted": quotation,
            "interpretation": proposal.source.interpretation, "scope": "project"},
            "temporal": temporal, "applied": true});
        if request.category == PromotionCategory::RuledOut {
            // Approach and whole reason stay together in one exact quotation; no host split.
            meaning["ruledOut"] = json!({"speaker": "userAct", "scope": "project",
                "rejectionAndReason": quotation, "evidence": "absent: the user's own report"});
        }
        let context = KnowledgeContext {
            category,
            authority: KnowledgeAuthority::HumanDirect,
            scope_id: None,
            end_condition: None,
            source_sequence: previous.source_sequence,
            unit_ordinal: previous.unit_ordinal,
            group_id: Some(group_id.clone()),
            validity: KnowledgeValidity::Current,
            payload: Some(meaning.to_string()),
        };
        super::context_bounds::validate_context(&mut tx, &context).await?;
        let value = NewBlackboardEntry {
            project_id: project.to_string(),
            node_id: entry.value.node_id.clone(),
            kind,
            content: quoted.to_string(),
            structured_value: None,
            confidence: ConfidenceScore::from_basis_points(/*value*/ 10_000)?,
            verification: BlackboardVerification::Unverified,
            importance: BlackboardImportance::High,
            root_promotion: RootPromotion::Promoted,
            evidence: Vec::new(),
            premises: Vec::new(),
            provenance: BlackboardProvenance {
                kind: BlackboardProvenanceKind::User,
                source_id: seal.exact_source_locator.clone(),
            },
        };
        value.validate()?;
        // A forgotten wording is never restored by an Apply; only a fresh explicit add does.
        super::identity::check_activation(&mut tx, &value, Some(&context)).await?;
        if let Some(existing) = super::identity::direct_match(&mut tx, &value, &context).await?
            && existing != entry.id
        {
            return Err(BlackboardStoreError::EntryIdentityConflict(
                existing.to_string(),
            ));
        }
        let revision = next_revision(&mut tx, project, &entry.id, entry.revision, now).await?;
        super::write_revision(
            &mut tx,
            &entry.id,
            revision,
            &value,
            BlackboardEntryState::Active,
            /*superseded_by*/ None,
            now,
        )
        .await?;
        super::knowledge::write_context(&mut tx, project, &entry.id, revision, &context).await?;
        let preview = super::knowledge::bounded_preview(quoted).to_string();
        let reason = json!({"fromRevision": entry.revision, "category": category.as_str(),
            "length": quoted.len()})
        .to_string();
        let member = CaptureGroupMember {
            ordinal: 0,
            entry_id: Some(entry.id.to_string()),
            revision: Some(revision as u64),
            outcome: MemberOutcome::Saved,
            preview,
            reason: Some(reason),
        };
        let receipt = CaptureGroup {
            project_id: project.to_string(),
            group_id: group_id.clone(),
            thread_id: Some(admission.thread_id().to_string()),
            turn_id: None,
            kind: "promotion-v1".to_string(),
            declared_count: None,
            recognized: 1,
            saved: 1,
            members: vec![member],
            ..CaptureGroup::default()
        };
        record_group(&mut tx, &receipt, &request.action_id, &fingerprint, now).await?;
        super::knowledge::append_change(
            &mut tx,
            project,
            Some((&entry.id, revision)),
            &ChangeRecord {
                operation: ChangeOperation::Promoted,
                origin: ChangeOrigin::DirectControl,
                category,
                action_id: Some(request.action_id),
                thread_id: Some(admission.thread_id().to_string()),
                turn_id: None,
                group_id: Some(group_id),
                preview: quoted.to_string(),
            },
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(receipt)
    }

    /// The exact words an Apply of `entry_id` as `category` would make the user's, or the
    /// refusal it would return. Read-only, with the same checks as `promote_proposal`, so a
    /// client shows the user exactly what an Apply settles before offering it.
    pub async fn proposal_application(
        &self,
        project_id: &str,
        entry_id: &BlackboardEntryId,
        category: PromotionCategory,
    ) -> Result<String, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let entry = super::load_entry(&mut tx, project_id, entry_id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(entry_id.to_string()))?;
        if entry.state != BlackboardEntryState::Active {
            return Err(BlackboardStoreError::EntryNotActive(entry.id.to_string()));
        }
        let applicable = applicable(&mut tx, project_id, &entry, category).await?;
        tx.commit().await?;
        Ok(applicable.quoted)
    }

    /// Undoes one committed receipt: retires what it newly saved or proposed, and returns an
    /// applied proposal to its earlier non-applied revision. Members it found already present
    /// stay untouched. Any affected member changed since the receipt conflicts the whole
    /// group; nothing is partially undone. A retry of the same action returns its result.
    pub async fn undo_capture_group(
        &self,
        admission: &codex_state::ThreadProjectAdmission,
        group_id: &str,
        action_id: &str,
    ) -> Result<CaptureGroup, BlackboardStoreError> {
        let project = admission.project_id();
        if action_id.trim().is_empty()
            || action_id.len() > 128
            || group_id.is_empty()
            || group_id.len() > 512
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let fingerprint = digest(&json!([project, "undo", group_id]).to_string());
        let undo_id = format!("undo-{}", digest(&json!([project, action_id]).to_string()));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = recorded_action(&mut tx, project, action_id, &fingerprint).await? {
            tx.commit().await?;
            return Ok(receipt);
        }
        let target = super::source_group_read::capture_group_on(&mut tx, project, group_id)
            .await?
            .ok_or_else(|| BlackboardStoreError::EntryNotFound(format!("receipt {group_id}")))?;
        let restores = match target.kind.as_str() {
            "promotion-v1" => true,
            "source-v1" | "proposal-v1" => false,
            _ => return Err(BlackboardStoreError::InvalidSource),
        };
        let now = super::unix_timestamp_millis()?;
        let mut affected = Vec::new();
        for member in &target.members {
            let (Some(id), Some(revision)) = (&member.entry_id, member.revision) else {
                continue;
            };
            if !matches!(
                member.outcome,
                MemberOutcome::Saved | MemberOutcome::Pending
            ) {
                continue;
            }
            let id = BlackboardEntryId::parse(id.clone())?;
            let entry = super::load_entry(&mut tx, project, &id)
                .await?
                .ok_or_else(|| BlackboardStoreError::EntryNotFound(id.to_string()))?;
            if entry.revision != revision || entry.state != BlackboardEntryState::Active {
                return Err(BlackboardStoreError::RevisionConflict {
                    expected: revision,
                    actual: entry.revision,
                });
            }
            affected.push((member.ordinal, entry));
        }
        let mut receipt = CaptureGroup {
            project_id: project.to_string(),
            group_id: undo_id.clone(),
            thread_id: Some(admission.thread_id().to_string()),
            turn_id: None,
            kind: "undo-v1".to_string(),
            declared_count: None,
            recognized: target.members.len() as u32,
            ..CaptureGroup::default()
        };
        for member in &target.members {
            let undone = affected
                .iter()
                .position(|(ordinal, _)| *ordinal == member.ordinal);
            let Some(index) = undone else {
                receipt.already_present += 1;
                let length = member
                    .reason
                    .as_deref()
                    .and_then(|reason| serde_json::from_str::<serde_json::Value>(reason).ok())
                    .and_then(|reason| reason.get("length").and_then(serde_json::Value::as_u64));
                receipt.members.push(CaptureGroupMember {
                    reason: Some(json!({"undo": "untouched", "length": length}).to_string()),
                    ..member.clone()
                });
                continue;
            };
            let entry = &affected[index].1;
            let revision = next_revision(&mut tx, project, &entry.id, entry.revision, now).await?;
            let (value, state, context, operation) = if restores {
                let (value, context) =
                    revision_meaning(&mut tx, project, &entry.id, entry.revision - 1).await?;
                (
                    value,
                    BlackboardEntryState::Active,
                    Some(context),
                    ChangeOperation::Invalidated,
                )
            } else {
                (
                    entry.value.clone(),
                    BlackboardEntryState::Tombstoned,
                    None,
                    ChangeOperation::Forgotten,
                )
            };
            super::write_revision(&mut tx, &entry.id, revision, &value, state, None, now).await?;
            if let Some(context) = &context {
                super::knowledge::write_context(&mut tx, project, &entry.id, revision, context)
                    .await?;
            }
            let category = match &context {
                Some(context) => context.category,
                None => super::knowledge::context_of(&mut tx, project, entry.id.as_str())
                    .await?
                    .map_or(KnowledgeCategory::Legacy, |context| context.category),
            };
            super::knowledge::append_change(
                &mut tx,
                project,
                Some((&entry.id, revision)),
                &ChangeRecord {
                    operation,
                    origin: ChangeOrigin::DirectControl,
                    category,
                    action_id: None,
                    thread_id: Some(admission.thread_id().to_string()),
                    turn_id: None,
                    group_id: Some(undo_id.clone()),
                    preview: value.content.clone(),
                },
                now,
            )
            .await?;
            receipt.saved += 1;
            receipt.members.push(CaptureGroupMember {
                ordinal: member.ordinal,
                entry_id: Some(entry.id.to_string()),
                revision: Some(revision as u64),
                outcome: MemberOutcome::Saved,
                preview: super::knowledge::bounded_preview(&value.content).to_string(),
                reason: Some(
                    json!({"undo": if restores { "proposalRestored" } else { "retired" },
                        "length": value.content.len()})
                    .to_string(),
                ),
            });
        }
        if receipt.saved == 0 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        record_group(&mut tx, &receipt, action_id, &fingerprint, now).await?;
        super::knowledge::append_change(
            &mut tx,
            project,
            /*entry*/ None,
            &ChangeRecord {
                operation: ChangeOperation::Forgotten,
                origin: ChangeOrigin::DirectControl,
                category: KnowledgeCategory::Note,
                action_id: Some(action_id.to_string()),
                thread_id: Some(admission.thread_id().to_string()),
                turn_id: None,
                group_id: Some(undo_id),
                preview: format!(
                    "receipt undone: {}",
                    super::knowledge::bounded_preview(group_id)
                ),
            },
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

/// What one proposal revision would apply, after every Apply check.
struct Applicable {
    previous: KnowledgeContext,
    payload: serde_json::Value,
    proposal: ProposalContext,
    seal: SourceSeal,
    quoted: String,
    quoted_start: u32,
    quoted_end: u32,
}

/// Only a complete, unqualified unit is applicable. An unresolved dependency (scope,
/// attribution, referent) keeps it a proposal for every category, and the applied words are
/// the whole bounded sealed part, which the proposal must cite in full: a partial citation
/// could drop a reason, negation or qualifier the user saw in the model's reading.
async fn applicable(
    tx: &mut SqliteConnection,
    project: &str,
    entry: &BlackboardEntry,
    category: PromotionCategory,
) -> Result<Applicable, BlackboardStoreError> {
    let previous = super::knowledge::context_of(tx, project, entry.id.as_str())
        .await?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    let payload: serde_json::Value = previous
        .payload
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| BlackboardStoreError::UnsupportedContext)?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    let proposal: ProposalContext = payload
        .get("proposal")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| BlackboardStoreError::UnsupportedContext)?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    let member: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM capture_group_members AS member JOIN capture_groups AS capture ON capture.project_id = member.project_id AND capture.group_id = member.group_id WHERE member.project_id = ? AND member.entry_id = ? AND capture.kind = 'proposal-v1')")
        .bind(project).bind(entry.id.as_str()).fetch_one(&mut *tx).await?;
    if !member
        || proposal.version != 1
        || entry.value.provenance.kind != BlackboardProvenanceKind::User
        || proposal.status != ProposalStatus::Proposed
        || proposal.source.dependency.is_some()
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    proposal.source.validate_bounds()?;
    // Someone else's words become a standing rule only through a reviewed delegation.
    if category == PromotionCategory::Rule
        && proposal
            .source
            .attribution
            .is_some_and(|attribution| attribution != ProposalAttribution::Unknown)
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let seal = super::source::seal_on(tx, project, &proposal.source.source_id)
        .await?
        .ok_or(BlackboardStoreError::InvalidSource)?;
    if seal.digest != proposal.source.digest
        || seal.observation.source_revision != proposal.source.source_revision
        || !seal.observation.complete_envelope
        || seal.original_utf8_length > MAX_APPLIED_BYTES
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let read = super::source::read_on(
        tx,
        project,
        &seal.exact_source_locator,
        &seal.digest,
        /*start*/ 0,
        seal.original_utf8_length,
    )
    .await?;
    if read.next_offset.is_some() {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let text = read.exact_text;
    let quoted_start = (text.len() - text.trim_start().len()) as u32;
    let quoted = text.trim().to_string();
    let quoted_end = quoted_start + quoted.len() as u32;
    let cited_start = proposal
        .source
        .spans
        .iter()
        .map(|span| span.start_byte)
        .min();
    let cited_end = proposal.source.spans.iter().map(|span| span.end_byte).max();
    if quoted.is_empty()
        || cited_start.is_none_or(|start| start > quoted_start)
        || cited_end.is_none_or(|end| end < quoted_end)
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    Ok(Applicable {
        previous,
        payload,
        proposal,
        seal,
        quoted,
        quoted_start,
        quoted_end,
    })
}

/// The receipt an identical earlier action committed, or a refusal when the action identity
/// was used for anything else.
async fn recorded_action(
    tx: &mut SqliteConnection,
    project: &str,
    action_id: &str,
    fingerprint: &str,
) -> Result<Option<CaptureGroup>, BlackboardStoreError> {
    let done: Option<(String, String)> = sqlx::query_as("SELECT group_id, request_fingerprint FROM capture_group_actions WHERE project_id = ? AND action_id = ?")
        .bind(project).bind(action_id).fetch_optional(&mut *tx).await?;
    if let Some((group_id, recorded)) = done {
        if recorded != fingerprint {
            return Err(BlackboardStoreError::ActionAlreadyRecorded(
                action_id.to_string(),
            ));
        }
        return super::source_group_read::capture_group_on(tx, project, &group_id).await;
    }
    let direct: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memory_changes WHERE project_id = ? AND action_id = ?)",
    )
    .bind(project)
    .bind(action_id)
    .fetch_one(&mut *tx)
    .await?;
    if direct {
        return Err(BlackboardStoreError::ActionAlreadyRecorded(
            action_id.to_string(),
        ));
    }
    Ok(None)
}

async fn next_revision(
    tx: &mut SqliteConnection,
    project: &str,
    id: &BlackboardEntryId,
    current: u64,
    now: i64,
) -> Result<i64, BlackboardStoreError> {
    let current = i64::try_from(current).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
    let next = current
        .checked_add(1)
        .ok_or(BlackboardStoreError::RevisionOverflow)?;
    let rows = sqlx::query(
        "UPDATE blackboard_entries SET revision = ?, updated_at_ms = ? WHERE project_id = ? AND id = ? AND revision = ?",
    )
    .bind(next)
    .bind(now)
    .bind(project)
    .bind(id.as_str())
    .bind(current)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if rows != 1 {
        return Err(BlackboardStoreError::ConcurrentMutation);
    }
    Ok(next)
}

/// Node, kind, confidence, verification, importance, promotion, provenance, source, content.
type StoredRevision = (
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
);
/// Category, authority, sequence, ordinal, group, validity, payload.
type StoredMeaning = (
    String,
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    String,
    Option<String>,
);

/// The stored value and meaning of an earlier proposal revision, read with bounded columns.
async fn revision_meaning(
    tx: &mut SqliteConnection,
    project: &str,
    id: &BlackboardEntryId,
    revision: u64,
) -> Result<(NewBlackboardEntry, KnowledgeContext), BlackboardStoreError> {
    let revision = i64::try_from(revision).map_err(|_| BlackboardStoreError::RevisionOverflow)?;
    let linked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blackboard_evidence_links WHERE entry_id = ? AND revision = ?) OR EXISTS(SELECT 1 FROM blackboard_premise_links WHERE entry_id = ? AND revision = ?)")
        .bind(id.as_str()).bind(revision).bind(id.as_str()).bind(revision).fetch_one(&mut *tx).await?;
    let row: Option<StoredRevision> = sqlx::query_as("SELECT entry.node_id, revision.kind, revision.confidence_basis_points, revision.verification, revision.importance, revision.root_promotion, revision.provenance_kind, revision.provenance_source_id, revision.content FROM blackboard_entry_revisions AS revision JOIN blackboard_entries AS entry ON entry.id = revision.entry_id WHERE entry.project_id = ? AND revision.entry_id = ? AND revision.revision = ? AND revision.state = 'active' AND revision.structured_value IS NULL AND octet_length(revision.content) <= 4096 AND octet_length(revision.provenance_source_id) <= 512")
        .bind(project).bind(id.as_str()).bind(revision).fetch_optional(&mut *tx).await?;
    let Some((
        node,
        kind,
        confidence,
        verification,
        importance,
        promotion,
        provenance,
        source,
        content,
    )) = row
    else {
        return Err(BlackboardStoreError::InvalidSource);
    };
    if linked {
        return Err(BlackboardStoreError::InvalidSource);
    }
    let value = NewBlackboardEntry {
        project_id: project.to_string(),
        node_id: HierarchyNodeId::parse(node).map_err(|_| BlackboardStoreError::InvalidSource)?,
        kind: super::parse_kind(&kind)?,
        content,
        structured_value: None,
        confidence: u16::try_from(confidence)
            .ok()
            .and_then(|value| ConfidenceScore::from_basis_points(value).ok())
            .ok_or(BlackboardStoreError::InvalidSource)?,
        verification: super::parse_verification(&verification)?,
        importance: super::parse_importance(&importance)?,
        root_promotion: super::parse_promotion(&promotion)?,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: super::parse_provenance(&provenance)?,
            source_id: source,
        },
    };
    value.validate()?;
    let context: Option<StoredMeaning> = sqlx::query_as("SELECT category, authority, source_sequence, unit_ordinal, group_id, validity, payload FROM knowledge_context WHERE project_id = ? AND entry_id = ? AND revision = (SELECT MAX(revision) FROM knowledge_context WHERE entry_id = ? AND revision <= ?) AND scope_id IS NULL AND end_condition IS NULL AND octet_length(category) <= 32 AND octet_length(authority) <= 32 AND octet_length(validity) <= 32 AND (group_id IS NULL OR octet_length(group_id) <= 512) AND (payload IS NULL OR octet_length(payload) <= 8192)")
        .bind(project).bind(id.as_str()).bind(id.as_str()).bind(revision).fetch_optional(&mut *tx).await?;
    let (category, authority, sequence, ordinal, group_id, validity, payload) =
        context.ok_or(BlackboardStoreError::InvalidSource)?;
    let context = KnowledgeContext {
        category: super::knowledge::parse(&category)?,
        authority: super::knowledge::parse(&authority)?,
        scope_id: None,
        end_condition: None,
        source_sequence: sequence
            .map(u64::try_from)
            .transpose()
            .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
        unit_ordinal: ordinal
            .map(u32::try_from)
            .transpose()
            .map_err(|_| BlackboardStoreError::RevisionOverflow)?,
        group_id,
        validity: super::knowledge::parse(&validity)?,
        payload,
    };
    // Only a non-applied proposal meaning is restored.
    if context.authority == KnowledgeAuthority::HumanDirect
        || value.root_promotion == RootPromotion::Promoted
        || context
            .payload
            .as_deref()
            .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
            .is_none_or(|payload| payload.get("proposal").is_none())
    {
        return Err(BlackboardStoreError::InvalidSource);
    }
    Ok((value, context))
}

async fn record_group(
    tx: &mut SqliteConnection,
    receipt: &CaptureGroup,
    action_id: &str,
    fingerprint: &str,
    now: i64,
) -> Result<(), BlackboardStoreError> {
    sqlx::query("INSERT INTO capture_groups(project_id, group_id, thread_id, turn_id, kind, declared_count, recognized, saved, already_present, pending, omitted, failed, recorded_at_ms) VALUES (?, ?, ?, ?, ?, NULL, ?, ?, ?, 0, 0, 0, ?)")
        .bind(&receipt.project_id).bind(&receipt.group_id).bind(&receipt.thread_id).bind(&receipt.turn_id).bind(&receipt.kind)
        .bind(i64::from(receipt.recognized)).bind(i64::from(receipt.saved)).bind(i64::from(receipt.already_present)).bind(now)
        .execute(&mut *tx).await?;
    for member in &receipt.members {
        if member
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > 240)
        {
            return Err(BlackboardStoreError::InvalidSource);
        }
        sqlx::query("INSERT INTO capture_group_members(project_id, group_id, ordinal, entry_id, revision, outcome, preview, reason) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&receipt.project_id).bind(&receipt.group_id).bind(i64::from(member.ordinal)).bind(&member.entry_id)
            .bind(member.revision.map(|revision| revision as i64)).bind(member.outcome.as_str()).bind(&member.preview).bind(&member.reason)
            .execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO capture_group_actions(project_id, action_id, group_id, request_fingerprint) VALUES (?, ?, ?, ?)")
        .bind(&receipt.project_id).bind(action_id).bind(&receipt.group_id).bind(fingerprint)
        .execute(&mut *tx).await?;
    Ok(())
}
