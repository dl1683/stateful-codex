//! Host capture of what a completed answer states under plain headings, with no model call:
//! each ruled-out item and each open check as its own entry, and each decision with the
//! reason written beside it (or marked as not recorded).
//!
//! The answer is read only when its turn completes; an interrupted or failed turn publishes
//! nothing. Entries carry the assistant's authority (they are what the answer reported, not
//! the user's word and not verified), the investigation the thread is bound to, their order
//! in the answer, and the answer's locator and digest. One counted receipt per kind reports
//! what was actually committed.

use codex_extension_api::ExtensionData;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::CaptureGroup;
use codex_project_intelligence::CaptureSource;
use codex_project_intelligence::CaptureUnit;
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
use codex_project_intelligence::ScopeState;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::answer_units::AnswerUnitKind;
use crate::answer_units::SourceText;
use crate::answer_units::answer_units;
use crate::events::CaptureOutcome;
use crate::events::GroupReceipt;
use crate::events::GroupReceiptItem;
use crate::events::KnowledgeCategory;
use crate::events::receipt_text;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::normalize;

/// Confidence recorded for what an answer reported: the assistant's account, unverified.
const ANSWER_CONFIDENCE_BASIS_POINTS: u16 = 6_000;
/// Longest answer opening kept with each unit, so recall can match the unit's topic.
const MAX_OPENING_BYTES: usize = 300;
/// Model-written entries compared against the answer's units for exact wording.
const MAX_RECONCILED_ENTRIES: u32 = 400;

/// The turn whose answer is being read.
pub(crate) struct CaptureTurn {
    pub(crate) turn_id: String,
}

/// The latest answer text of the turn that is not mid-turn commentary.
pub(crate) struct LatestAnswer {
    item_id: String,
    text: String,
}

/// Remembers the turn's latest non-commentary assistant message; the last one when the turn
/// completes is its final answer.
pub(crate) fn observe_agent_message(turn_store: &ExtensionData, message: &AgentMessageItem) {
    if message.phase == Some(MessagePhase::Commentary) {
        return;
    }
    let text = message
        .content
        .iter()
        .map(|content| match content {
            AgentMessageContent::Text { text } => text.as_str(),
        })
        .collect::<String>();
    turn_store.insert(LatestAnswer {
        item_id: message.id.clone(),
        text,
    });
}

/// Captures the units of a completed turn's final answer.
pub(crate) async fn capture_completed_answer(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    thread_id: &str,
    turn_store: &ExtensionData,
) {
    let (Some(turn), Some(answer)) = (
        turn_store.get::<CaptureTurn>(),
        turn_store.remove::<LatestAnswer>(),
    ) else {
        return;
    };
    let units = answer_units(&answer.text);
    if units.ruled_out.is_empty()
        && units.open_checks.is_empty()
        && units.decisions.is_empty()
        && units.omitted.is_empty()
    {
        return;
    }
    let source = Source {
        project_id,
        thread_id,
        turn_id: &turn.turn_id,
        capture: CaptureSource {
            locator: format!(
                "assistant-answer:{thread_id}/{}/{}",
                turn.turn_id, answer.item_id
            ),
            digest: format!("{:x}", Sha256::digest(answer.text.as_bytes())),
        },
        opening: opening_paragraph(&answer.text),
    };
    let store = match services.blackboard().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open the store for answer capture");
            return;
        }
    };
    let node_id = match services.project_node_id(project_id).await {
        Ok(node_id) => node_id,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to find the project for answer capture");
            return;
        }
    };
    let scope = match store.thread_scope(project_id, thread_id).await {
        Ok(Some(scope)) if scope.state == ScopeState::Open => Some((scope.scope_id, scope.title)),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to read the thread's investigation");
            None
        }
    };
    let source_sequence = store.allocate_source_sequence(project_id).await.ok();
    let placement = Placement {
        node_id,
        scope_id: scope.as_ref().map(|(scope_id, _)| scope_id.clone()),
        scope_title: scope.map(|(_, title)| title),
        source_sequence,
    };
    let omitted = |kind: AnswerUnitKind| {
        units
            .omitted
            .iter()
            .filter(move |(omitted, _)| *omitted == kind)
            .map(|(_, opening)| Planned::Omitted(format!("too long to keep whole: {opening}")))
    };
    let ruled_out = units
        .ruled_out
        .iter()
        .map(|item| Planned::Unit {
            content: item.text.clone(),
            payload: json!({ "source": span_json(&item.span) }),
        })
        .chain(omitted(AnswerUnitKind::RuledOut))
        .collect();
    let open_checks = units
        .open_checks
        .iter()
        .map(|item| Planned::Unit {
            content: item.text.clone(),
            payload: json!({
                "state": "open",
                "closesWhen": "the check as written is settled by recorded evidence",
                "source": span_json(&item.span),
            }),
        })
        .chain(omitted(AnswerUnitKind::OpenCheck))
        .collect();
    let decisions = units
        .decisions
        .iter()
        .map(|decision| Planned::Unit {
            content: decision.content(),
            payload: json!({
                "statedBy": "assistant answer (not a record of who chose or implemented it)",
                "choice": part_json(&decision.choice),
                "reason": decision.reason.as_ref().map(part_json),
                "reasonStatus": if decision.reason.is_some() { "recorded" } else { "notRecorded" },
                "alternatives": decision.alternatives.iter().map(part_json).collect::<Vec<_>>(),
                "reconsiderIf": decision.reconsider_if.as_ref().map(part_json),
                "source": span_json(&decision.span()),
            }),
        })
        .chain(omitted(AnswerUnitKind::Decision))
        .collect();
    for (kind, planned) in [
        (GroupKind::RULED_OUT, ruled_out),
        (GroupKind::OPEN_CHECKS, open_checks),
        (GroupKind::DECISIONS, decisions),
    ] {
        commit_group(store, event_sink, &source, &placement, kind, planned).await;
    }
}

/// One kind of group an answer can produce.
struct GroupKind {
    name: &'static str,
    category: Category,
    receipt: KnowledgeCategory,
    /// The blackboard kind model writes of the same knowledge use.
    entry_kind: BlackboardKind,
}

impl GroupKind {
    const RULED_OUT: Self = Self {
        name: "ruledOut",
        category: Category::RuledOut,
        receipt: KnowledgeCategory::RuledOut,
        entry_kind: BlackboardKind::RejectedApproach,
    };
    const OPEN_CHECKS: Self = Self {
        name: "openChecks",
        category: Category::OpenCheck,
        receipt: KnowledgeCategory::OpenCheck,
        entry_kind: BlackboardKind::Question,
    };
    const DECISIONS: Self = Self {
        name: "decisions",
        category: Category::Decision,
        receipt: KnowledgeCategory::Decision,
        entry_kind: BlackboardKind::Decision,
    };
}

struct Source<'a> {
    project_id: &'a str,
    thread_id: &'a str,
    turn_id: &'a str,
    capture: CaptureSource,
    /// The answer's opening paragraph: what its lists are about ("Root cause found: ...").
    opening: String,
}

struct Placement {
    node_id: HierarchyNodeId,
    scope_id: Option<String>,
    scope_title: Option<String>,
    source_sequence: Option<u64>,
}

enum Planned {
    Unit { content: String, payload: Value },
    Omitted(String),
}

async fn commit_group(
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
    // entry when this thread works in an investigation) is the same unit, not a second one.
    let existing = store
        .categorized_entries(
            source.project_id,
            &[category],
            &[entry_kind],
            MAX_RECONCILED_ENTRIES,
        )
        .await
        .map(|(entries, _)| entries)
        .unwrap_or_default();
    let mut units = Vec::with_capacity(planned.len());
    for (ordinal, planned) in planned.into_iter().enumerate() {
        let (content, payload) = match planned {
            Planned::Unit { content, payload } => (content, payload),
            Planned::Omitted(note) => {
                units.push(CaptureUnit::Omitted { note });
                continue;
            }
        };
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
                    "details": payload,
                })
                .to_string(),
            ),
            ..KnowledgeContext::new(category, KnowledgeAuthority::AssistantReported)
        };
        let words = normalize(&content);
        if let Some(same) = existing.iter().find(|candidate| {
            let applies = match &candidate.context {
                Some(other) => {
                    other.authority == KnowledgeAuthority::AssistantReported
                        && (other.scope_id.is_none() || other.scope_id == placement.scope_id)
                }
                None => candidate.entry.value.provenance.kind == BlackboardProvenanceKind::Agent,
            };
            applies && normalize(&candidate.entry.value.content) == words
        }) {
            units.push(CaptureUnit::Existing {
                id: same.entry.id.clone(),
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
        // Words forgotten project-wide stay forgotten inside an investigation too.
        let retired_identities = placement
            .scope_id
            .as_ref()
            .and_then(|_| unit_entry_id(source.project_id, category, None, &words))
            .into_iter()
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

/// The first paragraph of `answer`, at most `MAX_OPENING_BYTES`, cut on a character boundary.
fn opening_paragraph(answer: &str) -> String {
    let paragraph = answer
        .trim_start()
        .split(
            "

",
        )
        .next()
        .unwrap_or_default()
        .trim();
    let mut end = paragraph.len().min(MAX_OPENING_BYTES);
    while !paragraph.is_char_boundary(end) {
        end -= 1;
    }
    paragraph[..end].to_string()
}

fn span_json(span: &std::ops::Range<usize>) -> Value {
    json!({ "start": span.start, "end": span.end })
}

fn part_json(part: &SourceText) -> Value {
    json!({ "text": part.text, "start": part.span.start, "end": part.span.end })
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

#[cfg(test)]
#[path = "conversation_capture_tests.rs"]
mod tests;
