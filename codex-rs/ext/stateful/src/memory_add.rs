//! The user adds to project memory directly, with no model turn: a rule, something about
//! themselves, a decision with its reason, or a note. The entry carries the user's authority
//! because the request comes from the user's own client action, never from a model tool. Each
//! addition is journaled with its action identity, so a retried action returns what it did
//! and an action identity reused for different words is refused.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::CreateOutcome;
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
use crate::rule_capture::user_rule_entry_id;

/// Generations of one wording tried before giving up.
const MAX_GENERATIONS: u32 = 64;

/// What the user is adding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryAddition {
    /// A rule in the user's words. With a scope naming the open investigation the thread
    /// continues (its ID, its title, or "this investigation") the rule is limited to it and
    /// ends with it; any other scope is refused rather than applied to all work.
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
        // A scoped rule keeps the user's words; its investigation is recorded with it (and
        // shown by its title), not written into them.
        MemoryAddition::Rule { .. } => content.to_string(),
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
    let (kind, category) = match addition {
        MemoryAddition::Rule { .. } => (BlackboardKind::Instruction, KnowledgeCategory::Rule),
        MemoryAddition::Decision { .. } => (BlackboardKind::Decision, KnowledgeCategory::Decision),
        MemoryAddition::Background => (BlackboardKind::Fact, KnowledgeCategory::Background),
        MemoryAddition::Note => (BlackboardKind::Fact, KnowledgeCategory::Note),
    };
    // The complete request, so a retry is recognized by what was asked, not by stored text
    // that two different requests can share.
    let fingerprint = request_fingerprint(&addition, content);
    // A retried action returns what it did; the same identity for anything else (other
    // words, another kind, a different reason or scope) is refused.
    if let Some(done) = store.change_for_action(project_id, action_id).await? {
        if done.record.group_id.as_deref() != Some(fingerprint.as_str()) {
            return Err(MemoryControlError::Refused(
                "this action already added something else; nothing was added".to_string(),
            ));
        }
        let entry_id = done
            .entry_id
            .as_deref()
            .and_then(|id| BlackboardEntryId::parse(id).ok());
        let entry = match entry_id {
            Some(id) => store.get_entry(project_id, &id).await?,
            None => None,
        };
        return match entry {
            Some(entry)
                if entry.value.content == text
                    && entry.value.kind == kind
                    && done.record.category == category =>
            {
                Ok((entry, AddOutcome::AlreadyDone))
            }
            Some(_) | None => Err(MemoryControlError::Refused(
                "this action already added something else; nothing was added".to_string(),
            )),
        };
    }
    // A scoped rule belongs to the open investigation this thread continues, named by its ID
    // or its title, or as "this investigation"; it ends with that investigation.
    let scope_id = match &addition {
        MemoryAddition::Rule { scope } if let Some(named) = non_empty(scope) => {
            let thread_id = actor.thread_id.as_deref().ok_or_else(|| {
                MemoryControlError::Refused(
                    "a rule limited to an investigation needs the thread that continues it"
                        .to_string(),
                )
            })?;
            let bound = store
                .thread_scope(project_id, thread_id)
                .await?
                .filter(|scope| scope.state == codex_project_intelligence::ScopeState::Open)
                .ok_or_else(|| {
                    MemoryControlError::Refused(
                        "this thread continues no open investigation; join one (/memory investigations) or add the rule without a scope".to_string(),
                    )
                })?;
            let normalized = crate::user_rules::normalize(&named);
            let names_it = named == bound.scope_id
                || normalized == crate::user_rules::normalize(&bound.title)
                || CURRENT_INVESTIGATION.contains(&normalized.as_str());
            if !names_it {
                return Err(MemoryControlError::Refused(format!(
                    "that scope does not name the investigation this thread continues ({}); use its ID from /memory investigations or say \"this investigation\"",
                    bound.title
                )));
            }
            if normalized.split(' ').any(|word| word == "until") {
                return Err(MemoryControlError::Refused(
                    "a rule added directly ends with its investigation; leave out the ending"
                        .to_string(),
                ));
            }
            Some(bound.scope_id)
        }
        _ => None,
    };
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
                user_rule_entry_id(project_id, scope_id.as_deref(), &text, generation)
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
    let change = ChangeRecord {
        group_id: Some(fingerprint.clone()),
        ..actor.change(ChangeOperation::Saved, category, &text)
    };
    for id in candidates {
        let Some(existing) = store.get_entry(project_id, &id).await? else {
            let (entry, created) = store
                .create_entry_with_context(
                    id,
                    value,
                    KnowledgeContext {
                        scope_id,
                        ..KnowledgeContext::new(category, KnowledgeAuthority::HumanDirect)
                    },
                    change,
                )
                .await?;
            let outcome = match created {
                CreateOutcome::Created => AddOutcome::Added,
                CreateOutcome::AlreadyPresent => AddOutcome::AlreadyPresent,
            };
            return Ok((entry, outcome));
        };
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
                    Some(&ChangeRecord {
                        group_id: Some(fingerprint),
                        ..actor.change(ChangeOperation::Promoted, category, &text)
                    }),
                )
                .await?;
            return Ok((promoted, AddOutcome::Added));
        }
        // The action is journaled even though nothing changed, so a retry after the entry is
        // forgotten reports this outcome instead of adding the words again.
        store
            .record_change(
                project_id,
                Some(&existing),
                &ChangeRecord {
                    group_id: Some(fingerprint),
                    ..actor.change(ChangeOperation::Saved, category, &text)
                },
            )
            .await?;
        return Ok((existing, AddOutcome::AlreadyPresent));
    }
    Err(MemoryControlError::Refused(
        "these words were retired too many times to be added again".to_string(),
    ))
}

/// Ways to name the investigation the thread continues without its title or ID.
const CURRENT_INVESTIGATION: &[&str] = &[
    "this investigation",
    "this whole investigation",
    "the current investigation",
    "for this investigation",
    "for this whole investigation",
];

/// The journal's record of a direct addition's complete request (kind, words, reason,
/// scope), kept in the change's group field, which direct additions do not otherwise use.
fn request_fingerprint(addition: &MemoryAddition, content: &str) -> String {
    let (kind, extra) = match addition {
        MemoryAddition::Rule { scope } => ("rule", scope.as_deref()),
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
