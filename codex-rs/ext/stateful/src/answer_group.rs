//! One kind of answer unit committed as a counted group: the identity of each unit, its
//! reconciliation with entries already holding the same words (and with words the user
//! forgot), the atomic commit, and the receipt of what was actually committed.

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::CaptureGroup;
use codex_project_intelligence::CaptureSource;
use codex_project_intelligence::CaptureUnit;
use codex_project_intelligence::CategorizedEntry;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory as Category;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::MemberOutcome;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::events::CaptureOutcome;
use crate::events::GroupReceipt;
use crate::events::GroupReceiptItem;
use crate::events::KnowledgeCategory;
use crate::events::receipt_text;
use crate::user_rules::normalize;

/// Confidence recorded for what an answer reported: the assistant's account, unverified.
const ANSWER_CONFIDENCE_BASIS_POINTS: u16 = 6_000;

/// One kind of group an answer can produce.
pub(crate) struct GroupKind {
    pub(crate) name: &'static str,
    pub(crate) category: Category,
    pub(crate) receipt: KnowledgeCategory,
    /// The blackboard kind model writes of the same knowledge use.
    pub(crate) entry_kind: BlackboardKind,
}

impl GroupKind {
    pub(crate) const RULED_OUT: Self = Self {
        name: "ruledOut",
        category: Category::RuledOut,
        receipt: KnowledgeCategory::RuledOut,
        entry_kind: BlackboardKind::RejectedApproach,
    };
    pub(crate) const OPEN_CHECKS: Self = Self {
        name: "openChecks",
        category: Category::OpenCheck,
        receipt: KnowledgeCategory::OpenCheck,
        entry_kind: BlackboardKind::Question,
    };
    pub(crate) const DECISIONS: Self = Self {
        name: "decisions",
        category: Category::Decision,
        receipt: KnowledgeCategory::Decision,
        entry_kind: BlackboardKind::Decision,
    };
}

pub(crate) struct Source<'a> {
    pub(crate) project_id: &'a str,
    pub(crate) thread_id: &'a str,
    pub(crate) turn_id: &'a str,
    pub(crate) capture: CaptureSource,
    /// The answer's opening paragraph: what its lists are about ("Root cause found: ...").
    pub(crate) opening: String,
}

pub(crate) struct Placement {
    pub(crate) node_id: HierarchyNodeId,
    pub(crate) scope_id: Option<String>,
    pub(crate) scope_title: Option<String>,
    pub(crate) source_sequence: Option<u64>,
}

pub(crate) enum Planned {
    Unit { content: String, payload: Value },
    Omitted(String),
}

pub(crate) async fn commit_group(
    store: &BlackboardStore,
    event_sink: Option<&dyn StatefulEventSink>,
    source: &Source<'_>,
    placement: &Placement,
    kind: GroupKind,
    planned: Vec<Planned>,
) {
    if planned.is_empty() {
        return;
    }
    let GroupKind {
        name: group_kind,
        category,
        receipt: receipt_category,
        entry_kind,
    } = kind;
    let group_id = digest_id(
        "answer",
        &[
            source.project_id,
            source.thread_id,
            source.turn_id,
            group_kind,
        ],
    );
    // An entry already holding exactly this unit's words (a model write, or a project-wide
    // entry when this thread works in an investigation) is the same unit, not a second one;
    // the same words forgotten or replaced earlier, under any identity, stay forgotten.
    let applies = |candidate: &CategorizedEntry| match &candidate.context {
        Some(other) => {
            other.authority == KnowledgeAuthority::AssistantReported
                && (other.scope_id.is_none() || other.scope_id == placement.scope_id)
        }
        None => candidate.provenance_kind == BlackboardProvenanceKind::Agent,
    };
    let mut units = Vec::with_capacity(planned.len());
    for (ordinal, planned) in planned.into_iter().enumerate() {
        let (content, payload) = match planned {
            Planned::Unit { content, payload } => (content, payload),
            Planned::Omitted(note) => {
                units.push(CaptureUnit::Omitted { note });
                continue;
            }
        };
        let words = normalize(&content);
        let words_digest = format!("{:x}", Sha256::digest(words.as_bytes()));
        let context = KnowledgeContext {
            scope_id: placement.scope_id.clone(),
            source_sequence: placement.source_sequence,
            unit_ordinal: u32::try_from(ordinal).ok(),
            group_id: Some(group_id.clone()),
            payload: Some(
                json!({
                    "sourceLocator": source.capture.locator,
                    "sourceDigest": source.capture.digest,
                    "answerOpening": source.opening,
                    "wordsDigest": words_digest,
                    "details": payload,
                })
                .to_string(),
            ),
            ..KnowledgeContext::new(category, KnowledgeAuthority::AssistantReported)
        };
        let same_words = match store
            .entries_with_words(
                source.project_id,
                category,
                entry_kind,
                placement.scope_id.as_deref(),
                &content
                    .chars()
                    .filter(|character| !matches!(character, ' ' | '\t' | '\n' | '\r'))
                    .collect::<String>()
                    .to_ascii_lowercase(),
                &words_digest,
            )
            .await
        {
            Ok(entries) => entries
                .into_iter()
                .filter(|candidate| applies(candidate) && normalize(&candidate.content) == words)
                .collect::<Vec<_>>(),
            Err(error) => {
                tracing::warn!(project_id = %source.project_id, %error, "failed to read memory for an answer capture");
                return;
            }
        };
        // An open check is only ever created with its context, never attached to an entry
        // another writer could be changing, so its protection holds from its first revision.
        let reusable = category != Category::OpenCheck;
        if let Some(same) = same_words
            .iter()
            .find(|candidate| reusable && candidate.state == BlackboardEntryState::Active)
        {
            units.push(CaptureUnit::Existing {
                id: same.id.clone(),
                revision: same.revision,
                context,
            });
            continue;
        }
        let Some(id) = unit_entry_id(
            source.project_id,
            category,
            placement.scope_id.as_deref(),
            &words,
        ) else {
            units.push(CaptureUnit::Omitted {
                note: format!("not identifiable: {}", receipt_text(&content)),
            });
            continue;
        };
        let Ok(confidence) = ConfidenceScore::from_basis_points(ANSWER_CONFIDENCE_BASIS_POINTS)
        else {
            return;
        };
        // Words forgotten project-wide stay forgotten inside an investigation too, and so do
        // words forgotten under another identity (a model write this capture once matched).
        let retired_identities = placement
            .scope_id
            .as_ref()
            .and_then(|_| unit_entry_id(source.project_id, category, None, &words))
            .into_iter()
            .chain(
                same_words
                    .iter()
                    .filter(|candidate| candidate.state != BlackboardEntryState::Active)
                    .map(|candidate| candidate.id.clone()),
            )
            .collect();
        units.push(CaptureUnit::Entry {
            id,
            retired_identities,
            value: Box::new(NewBlackboardEntry {
                project_id: source.project_id.to_string(),
                node_id: placement.node_id.clone(),
                kind: entry_kind,
                content: content.clone(),
                structured_value: None,
                confidence,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Normal,
                root_promotion: RootPromotion::NotPromoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: source.capture.locator.clone(),
                },
            }),
            context,
            change: ChangeRecord {
                operation: ChangeOperation::Saved,
                origin: ChangeOrigin::HostCapture,
                category,
                action_id: None,
                thread_id: Some(source.thread_id.to_string()),
                turn_id: Some(source.turn_id.to_string()),
                group_id: Some(group_id.clone()),
                preview: content,
            },
        });
    }
    let group = CaptureGroup {
        project_id: source.project_id.to_string(),
        group_id: group_id.clone(),
        thread_id: Some(source.thread_id.to_string()),
        turn_id: Some(source.turn_id.to_string()),
        kind: group_kind.to_string(),
        ..CaptureGroup::default()
    };
    let recognized = u32::try_from(units.len()).unwrap_or(u32::MAX);
    let mut receipt = GroupReceipt {
        project_id: source.project_id.to_string(),
        thread_id: source.thread_id.to_string(),
        turn_id: source.turn_id.to_string(),
        group_id,
        category: receipt_category,
        declared_count: None,
        recognized,
        saved: 0,
        already_present: 0,
        pending: 0,
        omitted: 0,
        failed: 0,
        items: Vec::new(),
        omitted_items: Vec::new(),
        scope_title: placement.scope_title.clone(),
    };
    match store.commit_capture(&group, &source.capture, units).await {
        Ok((committed, entries)) => {
            if !committed.newly_committed {
                return;
            }
            receipt.saved = committed.group.saved;
            receipt.already_present = committed.group.already_present;
            receipt.omitted = committed.group.omitted;
            let mut entries = entries.into_iter();
            for member in &committed.members {
                match member.outcome {
                    MemberOutcome::Saved | MemberOutcome::AlreadyPresent => {
                        if let Some(entry) = entries.next() {
                            if let Some(event_sink) = event_sink {
                                event_sink.emit(StatefulEvent::BlackboardUpdated {
                                    project_id: source.project_id.to_string(),
                                    entity_kind: crate::BlackboardEntityKind::Entry,
                                    entity_id: entry.id.to_string(),
                                    revision: entry.revision,
                                });
                            }
                            receipt.items.push(GroupReceiptItem {
                                entry_id: entry.id.to_string(),
                                revision: entry.revision,
                                category: receipt_category,
                                outcome: if member.outcome == MemberOutcome::Saved {
                                    CaptureOutcome::Stored
                                } else {
                                    CaptureOutcome::AlreadyStored
                                },
                                text: receipt_text(&entry.value.content),
                            });
                        }
                    }
                    MemberOutcome::NotRestored => {
                        receipt.omitted_items.push(format!(
                            "forgotten earlier, not restored: {}",
                            receipt_text(member.note.as_deref().unwrap_or_default())
                        ));
                    }
                    MemberOutcome::Omitted => {
                        receipt
                            .omitted_items
                            .push(receipt_text(member.note.as_deref().unwrap_or_default()));
                    }
                }
            }
        }
        Err(error) => {
            // Nothing of the group was committed; say so instead of claiming any of it.
            tracing::warn!(project_id = %source.project_id, %error, "failed to commit an answer capture");
            receipt.failed = recognized;
        }
    }
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::KnowledgeGroupCaptured(receipt));
    }
}

/// Identity of an answer unit: its category, authority, scope and normalized words, so the
/// same item in a later answer is the same entry, and the same words in another
/// investigation are a different one.
fn unit_entry_id(
    project_id: &str,
    category: Category,
    scope_id: Option<&str>,
    words: &str,
) -> Option<BlackboardEntryId> {
    let digest = digest_id(
        "stateful-answer",
        &[
            project_id,
            category.as_str(),
            KnowledgeAuthority::AssistantReported.as_str(),
            scope_id.unwrap_or_default(),
            words,
        ],
    );
    BlackboardEntryId::parse(digest).ok()
}

fn digest_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &digest[..32])
}
