//! Host capture of the standing rules a user marks in their own message.
//!
//! Runs once at turn start on the turn's human-origin input, before the first sampling
//! step, so a rule stated in this message is already in this turn's project packet. No
//! model call is involved, and a rule that is stated again is not stored twice.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use sha2::Digest;
use sha2::Sha256;

use crate::BlackboardEntityKind;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;
use crate::user_rules::marked_rules;

/// Confidence recorded for a rule quoted from the user's own message.
const USER_RULE_CONFIDENCE_BASIS_POINTS: u16 = 10_000;

/// Where a captured user rule came from: its thread and turn.
pub(crate) fn user_message_source(thread_id: &str, turn_id: &str) -> String {
    format!("user-message:{thread_id}/{turn_id}")
}

/// Stable entry identity for a rule's exact wording within a project.
pub(crate) fn user_rule_entry_id(project_id: &str, clause: &str) -> Option<BlackboardEntryId> {
    let normalized = clause.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(normalized.as_bytes());
    BlackboardEntryId::parse(format!("stateful-user-rule-{:x}", hasher.finalize())).ok()
}

/// One user rule as the host recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapturedRule {
    pub(crate) entry: BlackboardEntry,
    pub(crate) standing: RuleStanding,
    /// False when the same wording was already recorded.
    pub(crate) newly_stored: bool,
}

/// Stores the rules `text` explicitly marks: standing ones promoted, pending ones (marked
/// as standing but also task-limited) as candidates that are never applied.
pub(crate) async fn capture_marked_rules(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    text: &str,
) -> Vec<CapturedRule> {
    let mut captured = Vec::new();
    for rule in marked_rules(text) {
        match store_user_rule(
            services,
            event_sink,
            project_id,
            thread_id,
            turn_id,
            &rule.text,
            rule.standing,
        )
        .await
        {
            Ok(rule) => captured.push(rule),
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to capture a user rule");
            }
        }
    }
    captured
}

/// Stores one rule in the user's exact words (the whole clause they wrote), or returns the
/// entry already holding that wording.
pub(crate) async fn store_user_rule(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    clause: &str,
    standing: RuleStanding,
) -> Result<CapturedRule, String> {
    let id = user_rule_entry_id(project_id, clause)
        .ok_or_else(|| "the rule cannot be identified".to_string())?;
    let store = services
        .blackboard()
        .await
        .map_err(|error| error.to_string())?;
    if let Some(existing) = store
        .get_entry(project_id, &id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(CapturedRule {
            entry: existing,
            standing,
            newly_stored: false,
        });
    }
    let node_id = services.project_node_id(project_id).await?;
    let confidence = ConfidenceScore::from_basis_points(USER_RULE_CONFIDENCE_BASIS_POINTS)
        .map_err(|error| error.to_string())?;
    let entry = store
        .create_entry(
            id,
            NewBlackboardEntry {
                project_id: project_id.to_string(),
                node_id,
                kind: BlackboardKind::Instruction,
                content: clause.to_string(),
                structured_value: None,
                confidence,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: match standing {
                    RuleStanding::Standing => RootPromotion::Promoted,
                    RuleStanding::Pending => RootPromotion::Candidate,
                },
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: user_message_source(thread_id, turn_id),
                },
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::BlackboardUpdated {
            project_id: project_id.to_string(),
            entity_kind: BlackboardEntityKind::Entry,
            entity_id: entry.id.to_string(),
            revision: entry.revision,
        });
    }
    Ok(CapturedRule {
        entry,
        standing,
        newly_stored: true,
    })
}

#[cfg(test)]
#[path = "rule_capture_tests.rs"]
mod tests;
