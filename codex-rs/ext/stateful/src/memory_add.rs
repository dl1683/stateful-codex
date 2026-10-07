//! The user adds to project memory directly, with no model turn: a rule, something about
//! themselves, a decision with its reason, or a note. The entry carries the user's authority
//! because the request comes from the user's own client action, never from a model tool. Each
//! addition is journaled with its action identity, so a retried action returns what it did
//! and an action identity reused for different words is refused.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use sha2::Digest;
use sha2::Sha256;

use crate::memory_controls::MAX_CORRECTION_BYTES;
use crate::memory_controls::MemoryActor;
use crate::memory_controls::MemoryControlError;
use crate::memory_controls::USER_BACKGROUND_ID_PREFIX;
use crate::rule_identity::user_rule_entry_id;

/// Generations of one wording tried before giving up.
const MAX_GENERATIONS: u32 = 64;

/// What the user is adding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryAddition {
    /// An unscoped rule. A legacy request supplying scope is refused.
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

/// Adds `content` to the project's memory as the user's own words. `actor.action_id`
/// identifies the user's action.
pub async fn add_entry(
    store: &BlackboardStore,
    actor: &MemoryActor,
    project_id: &str,
    node_id: HierarchyNodeId,
    addition: MemoryAddition,
    content: &str,
) -> Result<(BlackboardEntry, AddOutcome), MemoryControlError> {
    if matches!(&addition, MemoryAddition::Rule { scope: Some(_) }) {
        return Err(MemoryControlError::Refused(
            "investigations and scoped memory additions are unsupported; nothing was added"
                .to_string(),
        ));
    }
    let action_id = actor
        .action_id
        .as_deref()
        .filter(|action| !action.trim().is_empty())
        .ok_or_else(|| MemoryControlError::Refused("an action identity is required".to_string()))?;
    let content = content.trim();
    let non_empty = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let text = match &addition {
        MemoryAddition::Rule { .. } => content.to_string(),
        MemoryAddition::Decision { reason } => match non_empty(reason) {
            Some(reason) => format!("{content} Reason: {reason}"),
            None => content.to_string(),
        },
        MemoryAddition::Background | MemoryAddition::Note => content.to_string(),
    };
    if content.is_empty() || text.len() > MAX_CORRECTION_BYTES {
        return Err(MemoryControlError::Refused(format!(
            "the text (with its reason) must be 1-{MAX_CORRECTION_BYTES} bytes"
        )));
    }
    let (kind, category) = match addition {
        MemoryAddition::Rule { .. } => (BlackboardKind::Instruction, KnowledgeCategory::Rule),
        MemoryAddition::Decision { .. } => (BlackboardKind::Decision, KnowledgeCategory::Decision),
        MemoryAddition::Background => (BlackboardKind::Fact, KnowledgeCategory::Background),
        MemoryAddition::Note => (BlackboardKind::Fact, KnowledgeCategory::Note),
    };
    // The complete request, so a retry is recognized by what was asked, not by stored text
    // that two different requests can share.
    let fingerprint = request_fingerprint(&addition, content);
    let source_id = format!("memory-add:{action_id}");
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
        // Preserve historical identity for the same background words.
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
    let change = ChangeRecord {
        group_id: Some(fingerprint.clone()),
        ..actor.change(ChangeOperation::Saved, category, &text)
    };
    let result = store
        .write_capture(codex_project_intelligence::CaptureWrite {
            project_id: project_id.to_string(),
            group: None,
            scope: None,
            units: vec![codex_project_intelligence::CaptureUnitWrite::Entry(
                Box::new(codex_project_intelligence::CaptureEntryWrite {
                    candidates,
                    value,
                    context: KnowledgeContext {
                        ..KnowledgeContext::new(category, KnowledgeAuthority::HumanDirect)
                    },
                    change,
                    authority: codex_project_intelligence::CaptureAuthority::DirectAction,
                }),
            )],
        })
        .await;
    match result {
        Ok(result) => result
            .entries
            .into_iter()
            .next()
            .and_then(|(entry, outcome)| match outcome {
                codex_project_intelligence::MemberOutcome::AlreadyPresent => {
                    Some((entry, AddOutcome::AlreadyPresent))
                }
                codex_project_intelligence::MemberOutcome::Saved
                | codex_project_intelligence::MemberOutcome::Pending => {
                    Some((entry, AddOutcome::Added))
                }
                codex_project_intelligence::MemberOutcome::Omitted
                | codex_project_intelligence::MemberOutcome::Failed
                | codex_project_intelligence::MemberOutcome::NotRestored => None,
            })
            .ok_or_else(|| {
                MemoryControlError::Refused(
                    "these words were retired too many times to be added again".to_string(),
                )
            }),
        Err(codex_project_intelligence::BlackboardStoreError::ActionAlreadyRecorded(_)) => {
            // The losing transaction has rolled back. Replay only an identical request.
            let done = store
                .change_for_action(project_id, action_id)
                .await?
                .filter(|done| done.record.group_id.as_deref() == Some(fingerprint.as_str()))
                .ok_or_else(|| {
                    MemoryControlError::Refused(
                        "this action already added something else; nothing was added".to_string(),
                    )
                })?;
            let id = done
                .entry_id
                .and_then(|id| BlackboardEntryId::parse(id).ok())
                .ok_or_else(|| {
                    MemoryControlError::Refused("recorded action has no entry".to_string())
                })?;
            store
                .get_entry(project_id, &id)
                .await?
                .map(|entry| (entry, AddOutcome::AlreadyDone))
                .ok_or_else(|| {
                    MemoryControlError::Refused("recorded entry unavailable".to_string())
                })
        }
        Err(error) => Err(error.into()),
    }
}

/// The journal's record of a direct addition's complete request (kind, words, reason,
/// scope), kept in the change's group field, which direct additions do not otherwise use.
fn request_fingerprint(addition: &MemoryAddition, content: &str) -> String {
    let (kind, extra) = match addition {
        MemoryAddition::Rule { .. } => ("rule", None),
        MemoryAddition::Background => ("background", None),
        MemoryAddition::Decision { reason } => ("decision", reason.as_deref()),
        MemoryAddition::Note => ("note", None),
    };
    let mut hasher = Sha256::new();
    for part in [kind, content.trim(), extra.map_or("", str::trim)] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher.update([u8::from(extra.is_some())]);
    format!("add-request-{:x}", hasher.finalize())
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
