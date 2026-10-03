//! The user adds to project memory directly, with no model turn: a rule, something about
//! themselves, a decision with its reason, or a note. The entry carries the user's authority
//! because the request comes from the user's own client action, never from a model tool.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::CreateOutcome;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory as PiCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use sha2::Digest;
use sha2::Sha256;

use crate::memory_controls::ControlOrigin;
use crate::memory_controls::MAX_CORRECTION_BYTES;
use crate::memory_controls::MemoryControlError;
use crate::memory_controls::USER_BACKGROUND_ID_PREFIX;
use crate::rule_capture::user_rule_entry_id;

/// Generations of one wording tried before giving up.
const MAX_GENERATIONS: u32 = 8;

/// What the user is adding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryAddition {
    /// A rule in the user's words. With a scope ("this whole investigation into the config
    /// bug, until we agree on the root cause") it applies only there; the scope is kept in
    /// the user's words ahead of the rule, as host capture keeps a header's scope.
    Rule { scope: Option<String> },
    /// Something about the user or the whole work.
    Background,
    /// A decision, with its reason when the user gives one.
    Decision { reason: Option<String> },
    /// Anything else worth keeping.
    Note,
}

/// What an addition did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// A new entry was stored (or a kept, not-applied rule with these words now applies).
    Added,
    /// The same words were already current; nothing changed.
    AlreadyPresent,
    /// This action was already carried out; the entry it made is returned unchanged, even if
    /// it was forgotten since.
    AlreadyDone,
}

/// Adds `content` to the project's memory as the user's own words. `action_id` identifies
/// the user's action, so a retried request returns what the first one did.
pub async fn add_entry(
    store: &BlackboardStore,
    project_id: &str,
    node_id: HierarchyNodeId,
    addition: MemoryAddition,
    content: &str,
    action_id: &str,
    thread_id: &str,
) -> Result<(BlackboardEntry, AddOutcome), MemoryControlError> {
    let content = content.trim();
    let non_empty = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let text = match &addition {
        MemoryAddition::Rule { scope } => match non_empty(scope) {
            Some(scope) => format!("{}: {content}", scope.trim_end_matches(':')),
            None => content.to_string(),
        },
        MemoryAddition::Decision { reason } => match non_empty(reason) {
            Some(reason) => format!("{content} Reason: {reason}"),
            None => content.to_string(),
        },
        MemoryAddition::Background | MemoryAddition::Note => content.to_string(),
    };
    if content.is_empty() || text.len() > MAX_CORRECTION_BYTES {
        return Err(MemoryControlError::Refused(format!(
            "the text (with its scope or reason) must be 1-{MAX_CORRECTION_BYTES} bytes"
        )));
    }
    let source_id = format!("memory-add:{action_id}");
    let origin = ControlOrigin {
        thread_id: thread_id.to_string(),
        action_id: Some(action_id.to_string()),
    };
    let category = match addition {
        MemoryAddition::Rule { .. } => PiCategory::Rule,
        MemoryAddition::Decision { .. } => PiCategory::Decision,
        MemoryAddition::Background => PiCategory::Background,
        MemoryAddition::Note => PiCategory::Note,
    };
    let kind = match addition {
        MemoryAddition::Rule { .. } => BlackboardKind::Instruction,
        MemoryAddition::Decision { .. } => BlackboardKind::Decision,
        MemoryAddition::Background | MemoryAddition::Note => BlackboardKind::Fact,
    };
    let confidence = ConfidenceScore::from_basis_points(10_000)
        .map_err(|error| MemoryControlError::Refused(error.to_string()))?;
    let value = NewBlackboardEntry {
        project_id: project_id.to_string(),
        node_id,
        kind,
        content: text.clone(),
        structured_value: None,
        confidence,
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::User,
            source_id: source_id.clone(),
        },
    };
    let candidates = match addition {
        // A rule takes the identity of its wording, so stating it later finds this entry; a
        // direct addition is a fresh act of the user, so retired words come back under the
        // next generation.
        MemoryAddition::Rule { .. } => (0..MAX_GENERATIONS)
            .filter_map(|generation| {
                user_rule_entry_id(project_id, /*scope_id*/ None, &text, generation)
            })
            .collect(),
        // Background keeps the identity host capture gives the same words.
        MemoryAddition::Background => {
            let id = digest_id(USER_BACKGROUND_ID_PREFIX, project_id, &text)?;
            (0..MAX_GENERATIONS)
                .map(|generation| match generation {
                    0 => Ok(id.clone()),
                    generation => BlackboardEntryId::parse(format!("{id}-{generation}"))
                        .map_err(|error| MemoryControlError::Refused(error.to_string())),
                })
                .collect::<Result<_, _>>()?
        }
        MemoryAddition::Decision { .. } | MemoryAddition::Note => {
            vec![digest_id("stateful-memory-add-", project_id, action_id)?]
        }
    };
    for id in candidates {
        let Some(existing) = store.get_entry(project_id, &id).await? else {
            let stored = store
                .create_entry_with_context(
                    id.clone(),
                    value,
                    KnowledgeContext::new(category, KnowledgeAuthority::HumanDirect),
                    origin.change(ChangeOperation::Saved, category, &text),
                )
                .await;
            let (entry, created) = match stored {
                Ok(stored) => stored,
                // Another action stored the same words first; they are current.
                Err(BlackboardStoreError::EntryIdentityConflict(_)) => {
                    if let Some(existing) = store.get_entry(project_id, &id).await?
                        && existing.state == BlackboardEntryState::Active
                    {
                        return Ok((existing, AddOutcome::AlreadyPresent));
                    }
                    return Err(MemoryControlError::Refused(
                        "these words changed while they were being added; try again".to_string(),
                    ));
                }
                Err(error) => return Err(error.into()),
            };
            // A concurrent request of the same action may have stored it first.
            let outcome = match created {
                CreateOutcome::Created => AddOutcome::Added,
                CreateOutcome::AlreadyPresent => AddOutcome::AlreadyDone,
            };
            return Ok((entry, outcome));
        };
        // A retried action finds what it made, whatever happened to it since; it never
        // restores words forgotten after it.
        if existing.value.provenance.source_id == source_id {
            return Ok((existing, AddOutcome::AlreadyDone));
        }
        if existing.state != BlackboardEntryState::Active {
            continue;
        }
        // The user's direct rule applies even when the same words were kept as a
        // task-limited rule.
        if kind == BlackboardKind::Instruction
            && existing.value.root_promotion != RootPromotion::Promoted
        {
            let promoted = store
                .update_entry_recorded(
                    project_id,
                    &existing.id,
                    BlackboardEntryUpdate {
                        expected_revision: existing.revision,
                        kind: existing.value.kind,
                        content: existing.value.content.clone(),
                        structured_value: existing.value.structured_value.clone(),
                        confidence: existing.value.confidence,
                        verification: existing.value.verification,
                        importance: existing.value.importance,
                        root_promotion: RootPromotion::Promoted,
                        evidence: existing.value.evidence.clone(),
                        premises: existing.value.premises.clone(),
                        state: BlackboardEntryState::Active,
                        superseded_by: None,
                        provenance: BlackboardProvenance {
                            kind: BlackboardProvenanceKind::User,
                            source_id,
                        },
                    },
                    Some(&origin.change(
                        ChangeOperation::Promoted,
                        category,
                        &existing.value.content,
                    )),
                )
                .await?;
            return Ok((promoted, AddOutcome::Added));
        }
        return Ok((existing, AddOutcome::AlreadyPresent));
    }
    Err(MemoryControlError::Refused(
        "these words were retired too many times to be added again".to_string(),
    ))
}

fn digest_id(
    prefix: &str,
    project_id: &str,
    text: &str,
) -> Result<BlackboardEntryId, MemoryControlError> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(normalized.as_bytes());
    BlackboardEntryId::parse(format!("{prefix}{:x}", hasher.finalize()))
        .map_err(|error| MemoryControlError::Refused(error.to_string()))
}

#[cfg(test)]
#[path = "memory_add_tests.rs"]
mod tests;
