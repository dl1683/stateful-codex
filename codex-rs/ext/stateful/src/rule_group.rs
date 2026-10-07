//! Host capture of every rule one user message marks, as one counted group: rules keep the
//! order written, the rules a message gives an investigation share one scope that this thread
//! is bound to, and one receipt reports what was committed (saved, already saved, kept but not
//! applied, too long to keep, failed). The group and every member's outcome are stored, and a
//! repeated capture of the same message replays the first record instead of recounting.

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::CaptureGroup;
use codex_project_intelligence::CaptureGroupMember;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::KnowledgeCategory as ProjectCategory;
use codex_project_intelligence::KnowledgeScope;
use codex_project_intelligence::MemberOutcome;
use codex_project_intelligence::ScopeKind;
use codex_project_intelligence::ScopeState;
use sha2::Digest;
use sha2::Sha256;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::events::CaptureOutcome;
use crate::events::GroupReceipt;
use crate::events::GroupReceiptItem;
use crate::events::KnowledgeCategory;
use crate::events::receipt_text;
use crate::rule_capture::CapturedRule;
use crate::rule_capture::user_message_source;
use crate::rule_units::ScopeHint;
use crate::rule_units::marked_rule_units;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;

/// Stores the rules `text` marks (standing ones applied, task-limited ones kept but not
/// applied), binds this thread to the investigation the message opens, records the group,
/// and emits one counted receipt for it.
pub(crate) async fn capture_marked_rules(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    text: &str,
) -> Vec<CapturedRule> {
    let marked = marked_rule_units(text);
    if marked.rules.is_empty() && marked.omitted.is_empty() {
        return Vec::new();
    }
    let group_id = digest_id("rules", &[project_id, thread_id, turn_id]);
    let recognized = count(marked.rules.len() + marked.omitted.len());
    let group = CaptureGroup {
        project_id: project_id.to_string(),
        group_id: group_id.clone(),
        thread_id: Some(thread_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        kind: "rules".to_string(),
        declared_count: marked.declared_count,
        recognized,
        ..CaptureGroup::default()
    };
    let store = match services.blackboard().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open the store for rule capture");
            return Vec::new();
        }
    };
    let after_change = match store
        .message_watermark(project_id, thread_id, turn_id)
        .await
    {
        Ok(position) => position,
        Err(error) => {
            tracing::warn!(%error, "capture source position is unknown");
            return Vec::new();
        }
    };
    let node_id = match services.project_node_id(project_id).await {
        Ok(node) => node,
        Err(error) => {
            tracing::warn!(%error, "capture node unavailable");
            return Vec::new();
        }
    };
    let hint = marked
        .rules
        .iter()
        .filter_map(|rule| rule.scope.clone())
        .reduce(|first, next| match first.end_condition {
            Some(_) => first,
            None => ScopeHint {
                end_condition: next.end_condition,
                ..first
            },
        });
    // A source-backed declared opening may bind its own thread. Later prose cannot join
    // an investigation; an already explicitly bound thread may add rules to that scope.
    let scope = match &hint {
        Some(hint) => {
            match investigation(store, project_id, thread_id, turn_id, hint, text).await {
                Ok(scope) => scope,
                Err(error) => {
                    tracing::warn!(%error, "capture scope unavailable");
                    return Vec::new();
                }
            }
        }
        None => None,
    };
    let confidence =
        match codex_project_intelligence::ConfidenceScore::from_basis_points(/*value*/ 10_000) {
            Ok(confidence) => confidence,
            Err(error) => {
                tracing::warn!(%error, "invalid rule capture confidence");
                return Vec::new();
            }
        };
    let mut units = Vec::new();
    for (ordinal, rule) in marked.rules.into_iter().enumerate() {
        let scope_id = rule
            .scope
            .as_ref()
            .and(scope.as_ref())
            .map(|scope| scope.scope_id.clone());
        let context = codex_project_intelligence::KnowledgeContext {
            scope_id: scope_id.clone(),
            end_condition: rule
                .scope
                .as_ref()
                .and_then(|hint| hint.end_condition.clone()),
            unit_ordinal: Some(count(ordinal)),
            group_id: Some(group_id.clone()),
            ..codex_project_intelligence::KnowledgeContext::new(
                ProjectCategory::Rule,
                codex_project_intelligence::KnowledgeAuthority::HumanDirect,
            )
        };
        let candidates = (0..8)
            .filter_map(|generation| {
                crate::rule_capture::user_rule_entry_id(
                    project_id,
                    scope_id.as_deref(),
                    &rule.clause.text,
                    generation,
                )
            })
            .collect();
        units.push(codex_project_intelligence::CaptureUnitWrite::Entry(
            Box::new(codex_project_intelligence::CaptureEntryWrite {
                candidates,
                value: codex_project_intelligence::NewBlackboardEntry {
                    project_id: project_id.to_string(),
                    node_id: node_id.clone(),
                    kind: codex_project_intelligence::BlackboardKind::Instruction,
                    content: rule.clause.text.clone(),
                    structured_value: None,
                    confidence,
                    verification: codex_project_intelligence::BlackboardVerification::Unverified,
                    importance: codex_project_intelligence::BlackboardImportance::High,
                    root_promotion: match rule.clause.standing {
                        RuleStanding::Standing => {
                            codex_project_intelligence::RootPromotion::Promoted
                        }
                        RuleStanding::Pending => {
                            codex_project_intelligence::RootPromotion::Candidate
                        }
                    },
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    provenance: codex_project_intelligence::BlackboardProvenance {
                        kind: codex_project_intelligence::BlackboardProvenanceKind::User,
                        source_id: user_message_source(thread_id, turn_id),
                    },
                },
                context,
                change: ChangeRecord {
                    operation: ChangeOperation::Saved,
                    origin: ChangeOrigin::HostCapture,
                    category: ProjectCategory::Rule,
                    action_id: None,
                    thread_id: Some(thread_id.to_string()),
                    turn_id: Some(turn_id.to_string()),
                    group_id: Some(group_id.clone()),
                    preview: rule.clause.text,
                },
                authority: codex_project_intelligence::CaptureAuthority::Message {
                    after_change,
                    stated_at_ms: crate::rule_capture::now_ms(),
                },
            }),
        ));
    }
    for omitted in &marked.omitted {
        units.push(codex_project_intelligence::CaptureUnitWrite::Outcome(
            member(
                count(units.len()),
                MemberOutcome::Omitted,
                omitted,
                Some("too long to keep whole"),
            ),
        ));
    }
    let result = match store
        .write_capture(codex_project_intelligence::CaptureWrite {
            project_id: project_id.to_string(),
            group: Some(group),
            scope,
            units,
        })
        .await
    {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!(%project_id, %error, "rule capture rolled back; no receipt emitted");
            return Vec::new();
        }
    };
    if let Some(group) = result.group {
        let title = recorded_scope_title(store, project_id, &group).await;
        emit(event_sink, receipt(&group, title));
    }
    result
        .entries
        .into_iter()
        .map(|(entry, outcome)| CapturedRule {
            standing: if entry.value.root_promotion
                == codex_project_intelligence::RootPromotion::Promoted
            {
                RuleStanding::Standing
            } else {
                RuleStanding::Pending
            },
            entry,
            newly_stored: outcome != MemberOutcome::AlreadyPresent,
        })
        .collect()
}

/// Words with which a message starts an investigation other than the one its thread continues.
const ANOTHER_INVESTIGATION: &[&str] = &[
    "new investigation",
    "another investigation",
    "separate investigation",
    "different investigation",
];

/// The investigation the rules of this message belong to: the open one the thread continues
/// ("During this investigation, never push" adds to it), unless the message starts another;
/// otherwise the one this message opens, to which the thread is then bound.
async fn investigation(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    hint: &ScopeHint,
    text: &str,
) -> Result<Option<KnowledgeScope>, String> {
    let scope_id = scope_id_for(project_id, thread_id, turn_id, &hint.title);
    // Anywhere in the message: "Start a new investigation into parser.rs. Some ground rules
    // for this whole investigation: ..." opens a new one.
    let starts_another = {
        let message = text.to_lowercase();
        ANOTHER_INVESTIGATION
            .iter()
            .any(|phrase| message.contains(phrase))
    };
    if !starts_another
        && let Some(bound) = store
            .thread_scope(project_id, thread_id)
            .await
            .map_err(|error| error.to_string())?
            .filter(|scope| scope.state == ScopeState::Open && scope.scope_id != scope_id)
    {
        return Ok(Some(bound));
    }
    Ok(Some(KnowledgeScope {
        project_id: project_id.to_string(),
        scope_id,
        kind: ScopeKind::Investigation,
        title: hint.title.clone(),
        state: ScopeState::Open,
        end_condition: hint.end_condition.clone(),
        opened_source: user_message_source(thread_id, turn_id),
        ended_source: None,
        created_at_ms: 0,
        updated_at_ms: 0,
    }))
}

/// The investigation a recorded group's rules were saved under, read back from the first
/// stored member's recorded meaning.
async fn recorded_scope_title(
    store: &BlackboardStore,
    project_id: &str,
    group: &CaptureGroup,
) -> Option<String> {
    let ids = group
        .members
        .iter()
        .filter_map(|member| member.entry_id.clone())
        .map(BlackboardEntryId::parse)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let contexts = store.knowledge_contexts(project_id, &ids).await.ok()?;
    let scope_id = ids
        .iter()
        .find_map(|id| contexts.get(id.as_str())?.scope_id.clone())?;
    Some(store.scope(project_id, &scope_id).await.ok()??.title)
}

fn member(
    ordinal: u32,
    outcome: MemberOutcome,
    preview: &str,
    reason: Option<&str>,
) -> CaptureGroupMember {
    CaptureGroupMember {
        ordinal,
        entry_id: None,
        revision: None,
        outcome,
        preview: receipt_text(preview),
        reason: reason.map(str::to_string),
    }
}

/// The receipt for a recorded group: counts as committed, members in the order written.
fn receipt(group: &CaptureGroup, scope_title: Option<String>) -> GroupReceipt {
    GroupReceipt {
        project_id: group.project_id.clone(),
        thread_id: group.thread_id.clone().unwrap_or_default(),
        turn_id: group.turn_id.clone().unwrap_or_default(),
        group_id: group.group_id.clone(),
        category: KnowledgeCategory::Rule,
        declared_count: group.declared_count,
        recognized: group.recognized,
        saved: group.saved,
        already_present: group.already_present,
        pending: group.pending,
        omitted: group.omitted,
        failed: group.failed,
        items: group
            .members
            .iter()
            .filter_map(|member| {
                let (category, outcome) = match member.outcome {
                    MemberOutcome::Saved => (KnowledgeCategory::Rule, CaptureOutcome::Stored),
                    MemberOutcome::Pending => {
                        (KnowledgeCategory::PendingRule, CaptureOutcome::Stored)
                    }
                    MemberOutcome::AlreadyPresent => {
                        (KnowledgeCategory::Rule, CaptureOutcome::AlreadyStored)
                    }
                    MemberOutcome::Omitted | MemberOutcome::Failed | MemberOutcome::NotRestored => {
                        return None;
                    }
                };
                Some(GroupReceiptItem {
                    entry_id: member.entry_id.clone()?,
                    revision: member.revision?,
                    category,
                    outcome,
                    text: member.preview.clone(),
                })
            })
            .collect(),
        omitted_items: group
            .members
            .iter()
            .filter(|member| member.outcome == MemberOutcome::Omitted)
            .map(|member| member.preview.clone())
            .collect(),
        scope_title,
    }
}

fn emit(event_sink: Option<&dyn StatefulEventSink>, receipt: GroupReceipt) {
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::KnowledgeGroupCaptured(receipt));
    }
}

/// The investigation host capture gave the rules of a user message that names one: the scope
/// that message opened, or else the open one its thread continues. None when neither exists,
/// so the rule is not stored as one for all work.
pub(crate) async fn message_scope_id(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    text: &str,
) -> Option<String> {
    let hint = marked_rule_units(text)
        .rules
        .into_iter()
        .find_map(|rule| rule.scope)?;
    let opened = scope_id_for(project_id, thread_id, turn_id, &hint.title);
    if let Ok(Some(_)) = store.scope(project_id, &opened).await {
        return Some(opened);
    }
    store
        .thread_scope(project_id, thread_id)
        .await
        .ok()
        .flatten()
        .filter(|scope| scope.state == ScopeState::Open)
        .map(|scope| scope.scope_id)
}

/// The scope an investigation named in one user message gets; host capture and the model's
/// quote of the same message agree on it.
pub(crate) fn scope_id_for(
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    title: &str,
) -> String {
    digest_id("scope", &[project_id, thread_id, turn_id, title])
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
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
