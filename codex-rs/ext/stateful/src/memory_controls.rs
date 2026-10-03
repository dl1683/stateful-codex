//! The user's direct controls over project memory: forget an entry, or correct it in their
//! own words. No model call is involved; every change is checked against the revision the
//! user saw and kept as history.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::Succession;
use codex_project_intelligence::SupersededEntry;
use sha2::Digest;
use sha2::Sha256;

use crate::rule_capture::user_rule_entry_id;

/// Longest corrected text accepted, in bytes.
pub const MAX_CORRECTION_BYTES: usize = 2_000;

/// Generations of one rule wording tried before giving up.
const MAX_RULE_GENERATIONS: u32 = 8;

/// Where an active entry belongs when the user reviews memory, matching what new work
/// applies: only rules in the user's own words that are promoted are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySection {
    /// The user's standing rule, applied to new work.
    UserRule,
    /// The user's rule limited to a task, kept but never applied.
    PendingRule,
    /// A rule not in the user's own words, never applied.
    UnverifiedRule,
    Decision,
    Knowledge,
}

/// The section of an active entry.
pub fn memory_section(entry: &BlackboardEntry) -> MemorySection {
    match (entry.value.kind, entry.value.provenance.kind) {
        (BlackboardKind::Instruction, BlackboardProvenanceKind::User) => {
            if entry.value.root_promotion == RootPromotion::Promoted {
                MemorySection::UserRule
            } else {
                MemorySection::PendingRule
            }
        }
        (BlackboardKind::Instruction, _) => MemorySection::UnverifiedRule,
        (BlackboardKind::Decision, _) => MemorySection::Decision,
        _ => MemorySection::Knowledge,
    }
}

/// Why a control changed nothing.
#[derive(Debug)]
pub enum MemoryControlError {
    /// The request cannot apply as asked; the message says why.
    Refused(String),
    Store(BlackboardStoreError),
}

impl From<BlackboardStoreError> for MemoryControlError {
    fn from(error: BlackboardStoreError) -> Self {
        Self::Store(error)
    }
}

/// Retires the entry at the revision the user saw. Its text stays in history, and quoting
/// the message that first stated a retired rule cannot bring it back.
pub async fn forget_entry(
    store: &BlackboardStore,
    project_id: &str,
    id: &BlackboardEntryId,
    expected_revision: u64,
) -> Result<BlackboardEntry, MemoryControlError> {
    let current = current_entry(store, project_id, id).await?;
    if current.revision != expected_revision {
        return Err(conflict(expected_revision, current.revision));
    }
    if current.state != BlackboardEntryState::Active {
        return Err(MemoryControlError::Refused(format!(
            "entry {id} is already retired or replaced"
        )));
    }
    // Authorship is unchanged: the retired text is still attributed to whoever wrote it.
    Ok(store
        .update_entry(
            project_id,
            id,
            BlackboardEntryUpdate {
                expected_revision,
                kind: current.value.kind,
                content: current.value.content,
                structured_value: current.value.structured_value,
                confidence: current.value.confidence,
                verification: current.value.verification,
                importance: current.value.importance,
                root_promotion: current.value.root_promotion,
                evidence: current.value.evidence,
                premises: current.value.premises,
                state: BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: current.value.provenance,
            },
        )
        .await?)
}

/// Replaces the entry, at the revision the user saw, with `content` in the user's words.
/// The correction keeps the entry's kind and section, except that correcting a rule not in
/// the user's words makes it the user's standing rule (the caller names that action). A
/// retry of the same correction returns the stored result.
pub async fn correct_entry(
    store: &BlackboardStore,
    project_id: &str,
    id: &BlackboardEntryId,
    expected_revision: u64,
    content: &str,
) -> Result<Succession, MemoryControlError> {
    let content = content.trim();
    if content.is_empty() || content.len() > MAX_CORRECTION_BYTES {
        return Err(MemoryControlError::Refused(format!(
            "the corrected text must be 1-{MAX_CORRECTION_BYTES} bytes"
        )));
    }
    let current = current_entry(store, project_id, id).await?;
    // A retry finds the entry already superseded once, by this correction.
    let retried = current.state == BlackboardEntryState::Superseded
        && current.revision == expected_revision.saturating_add(1);
    if current.revision != expected_revision && !retried {
        return Err(conflict(expected_revision, current.revision));
    }
    if current.state == BlackboardEntryState::Tombstoned {
        return Err(MemoryControlError::Refused(format!(
            "entry {id} is retired; there is nothing to correct"
        )));
    }
    if current.value.content == content {
        return Err(MemoryControlError::Refused(
            "the corrected text is the same as the current text".to_string(),
        ));
    }
    let root_promotion = match memory_section(&current) {
        MemorySection::UnverifiedRule => RootPromotion::Promoted,
        MemorySection::UserRule
        | MemorySection::PendingRule
        | MemorySection::Decision
        | MemorySection::Knowledge => current.value.root_promotion,
    };
    let confidence = ConfidenceScore::from_basis_points(10_000)
        .map_err(|error| MemoryControlError::Refused(error.to_string()))?;
    // Only the corrected words carry over: earlier values, evidence and premises described
    // the old text.
    let value = NewBlackboardEntry {
        project_id: project_id.to_string(),
        node_id: current.value.node_id.clone(),
        kind: current.value.kind,
        content: content.to_string(),
        structured_value: None,
        confidence,
        verification: BlackboardVerification::Unverified,
        importance: current.value.importance,
        root_promotion,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::User,
            source_id: format!("memory-correct:{id}:{expected_revision}"),
        },
    };
    let replaced = vec![SupersededEntry {
        id: id.clone(),
        expected_revision,
    }];
    if current.value.kind != BlackboardKind::Instruction {
        let mut hasher = Sha256::new();
        for part in [
            project_id,
            id.as_str(),
            &expected_revision.to_string(),
            content,
        ] {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
        let successor = BlackboardEntryId::parse(format!(
            "stateful-memory-correction-{:x}",
            hasher.finalize()
        ))
        .map_err(|error| MemoryControlError::Refused(error.to_string()))?;
        return Ok(store.create_successor(successor, value, replaced).await?);
    }
    // A corrected rule takes the identity of its new wording, so the user stating the same
    // words later finds it instead of storing it twice.
    for generation in 0..MAX_RULE_GENERATIONS {
        let candidate = user_rule_entry_id(project_id, content, generation).ok_or_else(|| {
            MemoryControlError::Refused("the corrected rule cannot be identified".to_string())
        })?;
        match store.get_entry(project_id, &candidate).await? {
            Some(existing) if existing.state != BlackboardEntryState::Active => continue,
            Some(_) => {
                return store
                    .create_successor(candidate, value, replaced)
                    .await
                    .map_err(|error| match error {
                        BlackboardStoreError::EntryIdentityConflict(_) => {
                            MemoryControlError::Refused(
                                "that wording is already a current rule; forget this entry instead"
                                    .to_string(),
                            )
                        }
                        error => MemoryControlError::Store(error),
                    });
            }
            None => return Ok(store.create_successor(candidate, value, replaced).await?),
        }
    }
    Err(MemoryControlError::Refused(
        "this wording was retired too many times to be stored again".to_string(),
    ))
}

async fn current_entry(
    store: &BlackboardStore,
    project_id: &str,
    id: &BlackboardEntryId,
) -> Result<BlackboardEntry, MemoryControlError> {
    store
        .get_entry(project_id, id)
        .await?
        .ok_or_else(|| MemoryControlError::Refused(format!("entry not found: {id}")))
}

fn conflict(expected: u64, actual: u64) -> MemoryControlError {
    MemoryControlError::Store(BlackboardStoreError::RevisionConflict { expected, actual })
}

#[cfg(test)]
#[path = "memory_controls_tests.rs"]
mod tests;
