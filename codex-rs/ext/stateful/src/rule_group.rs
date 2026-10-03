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
use crate::rule_capture::ReceiptStyle;
use crate::rule_capture::RulePlacement;
use crate::rule_capture::RuleSource;
use crate::rule_capture::store_user_rule;
use crate::rule_capture::user_message_source;
use crate::rule_units::ScopeHint;
use crate::rule_units::marked_rule_units;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;

/// Capture groups being recorded in this process.
static IN_FLIGHT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Holds one capture group for this process until dropped.
struct InFlight(String);

impl InFlight {
    /// Waits until no other capture of `group_id` runs in this process, then holds it.
    async fn hold(group_id: &str) -> Self {
        loop {
            {
                let mut in_flight = IN_FLIGHT
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !in_flight.iter().any(|held| held == group_id) {
                    in_flight.push(group_id.to_string());
                    return Self(group_id.to_string());
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|held| held != &self.0);
    }
}

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
    let mut group = CaptureGroup {
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
            // Nothing was saved; the receipt says so rather than staying silent.
            tracing::warn!(%project_id, %error, "failed to open the store for rule capture");
            group.failed = recognized;
            emit(event_sink, receipt(&group, /*scope_title*/ None));
            return Vec::new();
        }
    };
    // Captures of one message run one at a time in this process, so a concurrent capture of
    // the same message replays the first one's record instead of recording its saves as
    // already present.
    let _held = InFlight::hold(&group_id).await;
    // A repeated capture of the same message (a resumed turn) replays what it first saved.
    if let Ok(Some(stored)) = store.capture_group(project_id, &group_id).await {
        let scope_title = recorded_scope_title(store, project_id, &stored).await;
        emit(event_sink, receipt(&stored, scope_title));
        return Vec::new();
    }
    // The message's place among journaled changes, kept by a retried capture of it.
    let after_change = store
        .message_watermark(project_id, thread_id, turn_id)
        .await
        .map_err(|error| {
            tracing::warn!(%project_id, %error, "failed to read the message's journal position");
        })
        .ok();
    let stated_at_ms = crate::rule_capture::now_ms();
    let source_sequence = match store.allocate_source_sequence(project_id).await {
        Ok(sequence) => Some(sequence),
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to allocate a capture position");
            None
        }
    };
    // One investigation per message: the rules it limits to an investigation share it.
    // Its ending is the one the user stated, whichever of its rules states it.
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
    let scope = match &hint {
        Some(hint) => open_investigation(store, project_id, thread_id, turn_id, hint, text)
            .await
            .map_err(|error| {
                tracing::warn!(%project_id, %error, "failed to open an investigation scope");
            }),
        None => Ok(None),
    };
    let rule_count = marked.rules.len();
    let mut captured = Vec::new();
    for (ordinal, rule) in marked.rules.into_iter().enumerate() {
        let ordinal = count(ordinal);
        let preview = receipt_text(&rule.clause.text);
        let scope_id = match (&rule.scope, &scope) {
            (None, _) => None,
            (Some(_), Ok(Some(scope_id))) => Some(scope_id.clone()),
            // A rule whose investigation could not be recorded is not saved as a rule for
            // all work.
            (Some(_), Ok(None) | Err(())) => {
                group.failed += 1;
                group.members.push(member(
                    ordinal,
                    MemberOutcome::Failed,
                    &preview,
                    Some("its investigation could not be recorded"),
                ));
                continue;
            }
        };
        let placement = RulePlacement {
            origin: ChangeOrigin::HostCapture,
            scope_id,
            end_condition: rule
                .scope
                .as_ref()
                .and_then(|hint| hint.end_condition.clone()),
            source_sequence,
            unit_ordinal: Some(ordinal),
            group_id: Some(group_id.clone()),
            receipt: ReceiptStyle::Grouped,
        };
        match store_user_rule(
            services,
            event_sink,
            project_id,
            RuleSource {
                thread_id,
                turn_id,
                receipt_turn_id: turn_id,
                stated_at_ms,
                after_change,
                placement,
            },
            &rule.clause.text,
            rule.clause.standing,
        )
        .await
        {
            Ok(stored) => {
                let outcome = match (stored.newly_stored, stored.standing) {
                    (false, _) => {
                        group.already_present += 1;
                        MemberOutcome::AlreadyPresent
                    }
                    (true, RuleStanding::Standing) => {
                        group.saved += 1;
                        MemberOutcome::Saved
                    }
                    (true, RuleStanding::Pending) => {
                        group.pending += 1;
                        MemberOutcome::Pending
                    }
                };
                group.members.push(CaptureGroupMember {
                    ordinal,
                    entry_id: Some(stored.entry.id.to_string()),
                    revision: Some(stored.entry.revision),
                    outcome,
                    preview: receipt_text(&stored.entry.value.content),
                    reason: None,
                });
                captured.push(stored);
            }
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to capture a user rule");
                group.failed += 1;
                group.members.push(member(
                    ordinal,
                    MemberOutcome::Failed,
                    &preview,
                    Some(&error),
                ));
            }
        }
    }
    for (offset, omitted) in marked.omitted.iter().enumerate() {
        group.omitted += 1;
        group.members.push(member(
            count(rule_count + offset),
            MemberOutcome::Omitted,
            omitted,
            Some("too long to keep whole"),
        ));
    }
    match store.record_capture_group(&group).await {
        Ok(true) => {}
        // Another capture of the same message recorded first: its record is the receipt.
        Ok(false) => {
            if let Ok(Some(stored)) = store.capture_group(project_id, &group_id).await {
                group = stored;
            }
        }
        Err(error) => {
            // The rules above are committed, but the group is not: the journal says the
            // capture is incomplete, and no counted receipt claims a recorded group.
            tracing::warn!(%project_id, %error, "failed to record a rule capture group");
            let incomplete = ChangeRecord {
                operation: ChangeOperation::CaptureIncomplete,
                origin: ChangeOrigin::HostCapture,
                category: ProjectCategory::Rule,
                action_id: None,
                thread_id: Some(thread_id.to_string()),
                turn_id: Some(turn_id.to_string()),
                group_id: Some(group_id.clone()),
                preview: format!("{} of {} rules saved", captured.len(), group.recognized),
            };
            if let Err(error) = store.record_change(project_id, None, &incomplete).await {
                tracing::warn!(%project_id, %error, "failed to journal an incomplete capture");
            }
            return captured;
        }
    }
    emit(event_sink, receipt(&group, hint.map(|hint| hint.title)));
    captured
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
async fn open_investigation(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    hint: &ScopeHint,
    text: &str,
) -> Result<Option<String>, String> {
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
        return Ok(Some(bound.scope_id));
    }
    store
        .open_scope(&KnowledgeScope {
            project_id: project_id.to_string(),
            scope_id: scope_id.clone(),
            kind: ScopeKind::Investigation,
            title: hint.title.clone(),
            state: ScopeState::Open,
            end_condition: hint.end_condition.clone(),
            opened_source: user_message_source(thread_id, turn_id),
            ended_source: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        })
        .await
        .map_err(|error| error.to_string())?;
    store
        .bind_thread_scope(project_id, thread_id, &scope_id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(scope_id))
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
                    MemberOutcome::Omitted | MemberOutcome::Failed => return None,
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
