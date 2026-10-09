//! Same-turn model proposals use the common identity fence and PI writer transaction.
use super::BlackboardStore;
use super::BlackboardStoreError;
use super::source;
use crate::*;
use serde_json::json;
use std::collections::BTreeMap;

impl BlackboardStore {
    /// Returns bounded typed origin, never inferred from content or provenance labels.
    pub async fn proposal_context(
        &self,
        project: &str,
        id: &BlackboardEntryId,
    ) -> Result<Option<ProposalContext>, BlackboardStoreError> {
        let mut tx = self.pool.begin().await?;
        let context = super::context_bounds::context_of(&mut tx, project, id.as_str()).await?;
        let proposal: Option<ProposalContext> = context
            .and_then(|context| context.payload)
            .map(|payload| serde_json::from_str::<serde_json::Value>(&payload))
            .transpose()
            .map_err(|_| BlackboardStoreError::UnsupportedContext)?
            .and_then(|json| json.get("proposal").cloned())
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| BlackboardStoreError::UnsupportedContext)?;
        if let Some(proposal) = &proposal {
            if proposal.version != 1 {
                return Err(BlackboardStoreError::UnsupportedContext);
            }
            proposal
                .source
                .validate_bounds()
                .map_err(|_| BlackboardStoreError::UnsupportedContext)?;
        }
        tx.commit().await?;
        Ok(proposal)
    }

    /// No authority switches: the host derives identities, origin, enclosure and clocks.
    /// Source observation is independent; entries/outcomes/links/journal commit together.
    pub async fn propose_sources(
        &self,
        admission: &codex_state::ThreadProjectAdmission,
        node: HierarchyNodeId,
        turn: &str,
        proposals: Vec<SourceProposal>,
    ) -> Result<Vec<ProposalResult>, BlackboardStoreError> {
        if proposals.is_empty() || proposals.len() > 24 || turn.is_empty() || turn.len() > 512 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        for proposal in &proposals {
            proposal.validate_bounds()?;
        }
        let project = admission.project_id();
        let request = json!([
            project,
            admission.thread_id(),
            admission.binding_generation(),
            turn,
            node,
            proposals
        ])
        .to_string();
        if request.len() > 32768 {
            return Err(BlackboardStoreError::InvalidSource);
        }
        let fingerprint = source::digest(&request);
        let action = format!("proposal-{fingerprint}");
        let group_id = action.clone();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT group_id FROM capture_group_actions WHERE project_id = ? AND action_id = ?",
        )
        .bind(project)
        .bind(&action)
        .fetch_optional(&mut *tx)
        .await?;
        // Read and validate native chunks once per source across the whole operation.
        let mut sources = BTreeMap::new();
        let mut bytes = 0usize;
        let mut budget = source::SourceBudget::default();
        for proposal in &proposals {
            if sources.contains_key(&proposal.source_id) {
                continue;
            }
            if sources.len() == 8 {
                return Err(BlackboardStoreError::InvalidSource);
            }
            let seal = source::seal_on(&mut tx, project, &proposal.source_id)
                .await?
                .ok_or(BlackboardStoreError::InvalidSource)?;
            let observation = &seal.observation;
            if observation.authoritative_thread_id != admission.thread_id()
                || observation.turn_id != turn
                || observation.binding_generation != admission.binding_generation()
            {
                return Err(BlackboardStoreError::InvalidSource);
            }
            // A complete enclosing part is mandatory; a long source stays exactly searchable.
            let exact = if seal.observation.complete_envelope
                && bytes + seal.original_utf8_length as usize <= 16384
            {
                let mut exact = String::new();
                let mut offset = 0;
                while offset < seal.original_utf8_length {
                    let end = (offset + 4096).min(seal.original_utf8_length);
                    // Whole source chunks end at validated UTF-8 boundaries. Use their indexed ends
                    // instead of cutting a Unicode character at the page-size boundary.
                    let boundary: i64 = sqlx::query_scalar("SELECT MAX(end_byte) FROM capture_source_chunks WHERE source_id = ? AND start_byte <= ? AND end_byte <= ?")
                        .bind(&proposal.source_id).bind(i64::from(offset)).bind(i64::from(end))
                        .fetch_one(&mut *tx).await?;
                    let page = source::read_with_budget(
                        &mut tx,
                        project,
                        &proposal.source_id,
                        &seal.digest,
                        offset,
                        boundary as u32,
                        &mut budget,
                    )
                    .await?;
                    offset = page.end_byte;
                    exact.push_str(&page.exact_text);
                }
                if source::digest(&exact) != seal.digest {
                    return Err(BlackboardStoreError::InvalidSource);
                }
                bytes += exact.len();
                Some(exact)
            } else {
                None
            };
            sources.insert(proposal.source_id.clone(), (seal, exact));
        }
        let now = super::unix_timestamp_millis()?;
        let mut results = Vec::new();
        let mut prepared = Vec::new();
        for (ordinal, proposal) in proposals.iter().enumerate() {
            let (seal, exact) = &sources[&proposal.source_id];
            if seal.digest != proposal.digest
                || seal.observation.source_revision != proposal.source_revision
                || seal.observation.part_index != proposal.part_index
            {
                return Err(BlackboardStoreError::InvalidSource);
            }
            let mut previous = 0;
            for span in &proposal.spans {
                if span.start_byte < previous
                    || span.start_byte >= span.end_byte
                    || span.end_byte > seal.original_utf8_length
                {
                    return Err(BlackboardStoreError::InvalidSource);
                }
                previous = span.end_byte;
                if let Some(exact) = exact
                    && exact
                        .get(span.start_byte as usize..span.end_byte as usize)
                        .is_none()
                {
                    return Err(BlackboardStoreError::InvalidSource);
                }
            }
            let Some(exact) = exact else {
                results.push(ProposalResult {
                    entry_id: None,
                    revision: None,
                    status: ProposalStatus::Omitted,
                    source_id: proposal.source_id.clone(),
                    reason: Some(
                        "complete enclosure exceeds the group budget; exact source retained".into(),
                    ),
                });
                prepared.push(None);
                continue;
            };
            if let Some(index) = proposal.speaker_span {
                let span = proposal
                    .spans
                    .get(usize::from(index))
                    .ok_or(BlackboardStoreError::InvalidSource)?;
                if span.role != SourceSpanRole::Attribution || span.end_byte - span.start_byte > 160
                {
                    return Err(BlackboardStoreError::InvalidSource);
                }
            }
            let status = if proposal.dependency.is_some() {
                ProposalStatus::Pending
            } else {
                ProposalStatus::Proposed
            };
            let enclosure = SourceSpan {
                start_byte: 0,
                end_byte: seal.original_utf8_length,
                role: SourceSpanRole::Body,
            };
            let meaning = ProposalContext {
                version: 1,
                status,
                source: proposal.clone(),
                enclosure: enclosure.clone(),
            };
            let category = match proposal.category {
                ProposalCategory::Rule => KnowledgeCategory::Rule,
                ProposalCategory::Background => KnowledgeCategory::Background,
                ProposalCategory::AttributedContext => KnowledgeCategory::AttributedContext,
                ProposalCategory::Decision => KnowledgeCategory::Decision,
                ProposalCategory::BrainstormOption => KnowledgeCategory::BrainstormOption,
                ProposalCategory::RuledOut => KnowledgeCategory::RuledOut,
                ProposalCategory::OpenCheck => KnowledgeCategory::OpenCheck,
                ProposalCategory::Note => KnowledgeCategory::Note,
            };
            let mut context =
                KnowledgeContext::new(category, KnowledgeAuthority::AssistantReported);
            context.validity = KnowledgeValidity::NeedsCheck;
            context.source_sequence = Some(seal.immutable_first_observation_sequence);
            context.unit_ordinal = Some(ordinal as u32);
            context.group_id = Some(group_id.clone());
            let mut temporal = super::proposal_time::temporal(proposal, seal, exact)?;
            temporal.recorded_at_ms = now;
            context.payload = Some(json!({"proposal": meaning, "temporal": temporal,
                "attribution": "user-delivered material; proposal attribution is model-declared; see exact enclosure", "applied": false}).to_string());
            let id = BlackboardEntryId::parse(format!(
                "proposal-{}",
                source::digest(&format!("{fingerprint}:{ordinal}"))
            ))?;
            let value = NewBlackboardEntry {
                project_id: project.to_string(),
                node_id: node.clone(),
                kind: BlackboardKind::Note,
                content: proposal.interpretation.clone(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(/*value*/ 0)?,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Normal,
                root_promotion: RootPromotion::NotPromoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: seal.exact_source_locator.clone(),
                },
            };
            let write = CaptureEntryWrite {
                candidates: vec![id.clone()],
                value,
                context,
                change: ChangeRecord {
                    operation: ChangeOperation::CaptureIncomplete,
                    origin: ChangeOrigin::HostCapture,
                    category,
                    action_id: None,
                    thread_id: Some(admission.thread_id().to_string()),
                    turn_id: Some(turn.to_string()),
                    group_id: Some(group_id.clone()),
                    preview: "non-applied source proposal".into(),
                },
            };
            super::context_bounds::validate_context(&mut tx, &write.context).await?;
            results.push(ProposalResult {
                entry_id: Some(id.to_string()),
                revision: Some(1),
                status,
                source_id: proposal.source_id.clone(),
                reason: proposal
                    .dependency
                    .map(|dependency| format!("unresolved {dependency:?}")),
            });
            prepared.push(Some((write, enclosure)));
        }
        // A repeated source action reads the same committed outcome; it never mutates User entries.
        if let Some(existing) = existing {
            if existing != group_id {
                return Err(BlackboardStoreError::InvalidSource);
            }
            let receipt = super::source_group_read::capture_group_on(&mut tx, project, &group_id)
                .await?
                .ok_or(BlackboardStoreError::InvalidSource)?;
            if receipt.members.len() != results.len() {
                return Err(BlackboardStoreError::InvalidSource);
            }
            for result in &results {
                if let Some(id) = &result.entry_id {
                    let id = BlackboardEntryId::parse(id.clone())?;
                    let entry = super::load_entry(&mut tx, project, &id)
                        .await?
                        .ok_or(BlackboardStoreError::InvalidSource)?;
                    if entry.revision != 1
                        || entry.state != BlackboardEntryState::Active
                        || !super::identity::entry_storage_eligible_on(&mut tx, project, &id)
                            .await?
                    {
                        return Err(BlackboardStoreError::RetiredIdentity);
                    }
                }
            }
            tx.commit().await?;
            return Ok(results);
        }
        for (ordinal, item) in prepared.into_iter().enumerate() {
            let Some((write, enclosure)) = item else {
                continue;
            };
            // C2's no-reuse cut applies before the common writer can reconcile an existing entry.
            if let Some(id) =
                super::identity::direct_match(&mut tx, &write.value, &write.context).await?
            {
                return Err(BlackboardStoreError::EntryIdentityConflict(id.to_string()));
            }
            let id = write.candidates[0].clone();
            let Some((entry, MemberOutcome::Pending)) =
                super::capture_write::write_unit(&mut tx, write, now).await?
            else {
                return Err(BlackboardStoreError::InvalidSource);
            };
            if entry.id != id {
                return Err(BlackboardStoreError::InvalidSource);
            }
            sqlx::query("INSERT INTO capture_entry_sources(entry_id, source_id, start_byte, end_byte, role) VALUES (?, ?, ?, ?, ?)")
                .bind(id.as_str()).bind(&results[ordinal].source_id).bind(i64::from(enclosure.start_byte))
                .bind(i64::from(enclosure.end_byte)).bind("\"body\"").execute(&mut *tx).await?;
        }
        let omitted = results
            .iter()
            .filter(|result| result.status == ProposalStatus::Omitted)
            .count() as i64;
        sqlx::query("INSERT INTO capture_groups(project_id, group_id, thread_id, turn_id, kind, recognized, saved, already_present, pending, omitted, failed, recorded_at_ms) VALUES (?, ?, ?, ?, 'proposal-v1', ?, 0, 0, ?, ?, 0, ?)")
            .bind(project).bind(&group_id).bind(admission.thread_id()).bind(turn).bind(results.len() as i64)
            .bind(results.len() as i64 - omitted).bind(omitted).bind(now).execute(&mut *tx).await?;
        for (ordinal, result) in results.iter().enumerate() {
            // The existing outcome vocabulary has pending for all non-applied stored units;
            // exact proposed/pending/omitted meaning is retained in this bounded member payload.
            let outcome = if result.status == ProposalStatus::Omitted {
                "omitted"
            } else {
                "pending"
            };
            let reason =
                json!({"status":result.status,"sourceId":result.source_id,"reason":result.reason})
                    .to_string();
            if reason.len() > 240 {
                return Err(BlackboardStoreError::InvalidSource);
            }
            sqlx::query("INSERT INTO capture_group_members(project_id, group_id, ordinal, entry_id, revision, outcome, preview, reason) VALUES (?, ?, ?, ?, ?, ?, '', ?)")
                .bind(project).bind(&group_id).bind(ordinal as i64).bind(&result.entry_id).bind(result.revision.map(|revision| revision as i64))
                .bind(outcome).bind(reason).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO capture_group_actions(project_id, action_id, group_id, request_fingerprint) VALUES (?, ?, ?, ?)")
            .bind(project).bind(&action).bind(&group_id).bind(&fingerprint).execute(&mut *tx).await?;
        super::knowledge::append_change(
            &mut tx,
            project,
            /*entry*/ None,
            &ChangeRecord {
                operation: ChangeOperation::CaptureIncomplete,
                origin: ChangeOrigin::HostCapture,
                category: KnowledgeCategory::Note,
                action_id: Some(action),
                thread_id: Some(admission.thread_id().to_string()),
                turn_id: Some(turn.to_string()),
                group_id: Some(group_id),
                preview: "source proposals committed; none applied".into(),
            },
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(results)
    }
}

#[cfg(test)]
#[path = "proposal_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "proposal_product_tests.rs"]
mod product_tests;

#[cfg(test)]
#[path = "proposal_boundaries_tests.rs"]
mod boundary_tests;

#[cfg(test)]
#[path = "proposal_identity_tests.rs"]
mod identity_tests;
