//! Host capture of every rule one user message marks, as one counted group: rules keep the
//! order written, a rule for an investigation is bound to that investigation, and one receipt
//! reports what was actually committed (saved, already saved, kept but not applied, too long
//! to keep, failed).

use codex_project_intelligence::CaptureGroup;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::KnowledgeScope;
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
use crate::rule_units::marked_rule_units;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;

/// Stores the rules `text` marks (standing ones applied, task-limited ones kept but not
/// applied), binds this thread to an investigation the message opens, and emits one counted
/// receipt for the group.
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
    let source = user_message_source(thread_id, turn_id);
    let store = match services.blackboard().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open the store for rule capture");
            return Vec::new();
        }
    };
    let source_sequence = match store.allocate_source_sequence(project_id).await {
        Ok(sequence) => Some(sequence),
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to allocate a capture position");
            None
        }
    };
    let mut receipt = GroupReceipt {
        project_id: project_id.to_string(),
        thread_id: thread_id.to_string(),
        turn_id: turn_id.to_string(),
        group_id: group_id.clone(),
        category: KnowledgeCategory::Rule,
        declared_count: marked.declared_count,
        recognized: count(marked.rules.len() + marked.omitted.len()),
        saved: 0,
        already_present: 0,
        pending: 0,
        omitted: count(marked.omitted.len()),
        failed: 0,
        items: Vec::new(),
        omitted_items: marked.omitted.clone(),
        scope_title: None,
    };
    let mut captured = Vec::new();
    for (ordinal, rule) in marked.rules.into_iter().enumerate() {
        // An investigation the message names is opened once and this thread is bound to it,
        // so its rules apply here and in threads that continue it, not in unrelated work.
        let scope_id = match &rule.scope {
            Some(hint) => {
                let scope_id = scope_id_for(project_id, thread_id, turn_id, &hint.title);
                let opened = store
                    .open_scope(&KnowledgeScope {
                        project_id: project_id.to_string(),
                        scope_id: scope_id.clone(),
                        kind: ScopeKind::Investigation,
                        title: hint.title.clone(),
                        state: ScopeState::Open,
                        end_condition: hint.end_condition.clone(),
                        opened_source: source.clone(),
                        ended_source: None,
                        created_at_ms: 0,
                        updated_at_ms: 0,
                    })
                    .await;
                let bound = match opened {
                    Ok(_) => store
                        .bind_thread_scope(project_id, thread_id, &scope_id)
                        .await
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                };
                if let Err(error) = bound {
                    tracing::warn!(%project_id, %error, "failed to open an investigation scope");
                    receipt.failed += 1;
                    continue;
                }
                receipt.scope_title = Some(hint.title.clone());
                Some(scope_id)
            }
            None => None,
        };
        let placement = RulePlacement {
            origin: ChangeOrigin::HostCapture,
            scope_id,
            end_condition: rule
                .scope
                .as_ref()
                .and_then(|hint| hint.end_condition.clone()),
            source_sequence,
            unit_ordinal: u32::try_from(ordinal).ok(),
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
                // The message starting this turn follows every retirement recorded so far.
                stated_at_ms: i64::MAX,
                placement,
            },
            &rule.clause.text,
            rule.clause.standing,
        )
        .await
        {
            Ok(stored) => {
                let outcome = if stored.newly_stored {
                    CaptureOutcome::Stored
                } else {
                    CaptureOutcome::AlreadyStored
                };
                match (stored.newly_stored, stored.standing) {
                    (false, _) => receipt.already_present += 1,
                    (true, RuleStanding::Standing) => receipt.saved += 1,
                    (true, RuleStanding::Pending) => receipt.pending += 1,
                }
                receipt.items.push(GroupReceiptItem {
                    entry_id: stored.entry.id.to_string(),
                    revision: stored.entry.revision,
                    category: match stored.standing {
                        RuleStanding::Standing => KnowledgeCategory::Rule,
                        RuleStanding::Pending => KnowledgeCategory::PendingRule,
                    },
                    outcome,
                    text: receipt_text(&stored.entry.value.content),
                });
                captured.push(stored);
            }
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to capture a user rule");
                receipt.failed += 1;
            }
        }
    }
    let group = CaptureGroup {
        project_id: project_id.to_string(),
        group_id,
        thread_id: Some(thread_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        kind: "rules".to_string(),
        declared_count: receipt.declared_count,
        recognized: receipt.recognized,
        saved: receipt.saved,
        already_present: receipt.already_present,
        pending: receipt.pending,
        omitted: receipt.omitted,
        failed: receipt.failed,
    };
    if let Err(error) = store.record_capture_group(&group).await {
        tracing::warn!(%project_id, %error, "failed to record a rule capture group");
    }
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::KnowledgeGroupCaptured(receipt));
    }
    captured
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
