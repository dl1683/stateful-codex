//! Host capture of the standing rules a user marks in their own message.
//!
//! Runs once at turn start on the turn's human-origin input, before the first sampling
//! step, so a rule stated in this message is already in this turn's project packet. No
//! model call is involved, and a rule that is stated again is not stored twice.

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
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use sha2::Digest;
use sha2::Sha256;

use crate::BlackboardEntityKind;
use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::events::CaptureOutcome;
use crate::events::KnowledgeCategory;
use crate::events::receipt_text;
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
/// Stable identity for a rule's exact wording within a project. Retired or superseded
/// entries are immutable history, so restating such a rule stores it again under the next
/// generation of the same identity.
pub(crate) fn user_rule_entry_id(
    project_id: &str,
    clause: &str,
    generation: u32,
) -> Option<BlackboardEntryId> {
    let normalized = clause.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    let id = match generation {
        0 => format!("stateful-user-rule-{digest:x}"),
        generation => format!("stateful-user-rule-{digest:x}-{generation}"),
    };
    BlackboardEntryId::parse(id).ok()
}

/// The current time in Unix milliseconds, the clock entry timestamps use.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

const RETIRED_BEFORE_MESSAGE: &str = "nothing written: this rule was retired after the user wrote that message; only a later message from the user restores it";

/// Restatements of one wording after which a rule is no longer re-established.
const MAX_RULE_GENERATIONS: u32 = 8;

/// One user rule as the host recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapturedRule {
    pub(crate) entry: BlackboardEntry,
    pub(crate) standing: RuleStanding,
    /// False when the same wording was already recorded.
    pub(crate) newly_stored: bool,
}

/// Stores what the user says about themselves or the whole work ("I know Python well but
/// only a little Rust") verbatim, as promoted user-authored background. A statement already
/// stored is not stored twice, and one the user forgot stays forgotten.
pub(crate) async fn capture_background(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    text: &str,
) {
    for statement in crate::user_rules::background_statements(text) {
        if let Err(error) = store_background(
            services, event_sink, project_id, thread_id, turn_id, &statement,
        )
        .await
        {
            tracing::warn!(%project_id, %error, "failed to capture user background");
        }
    }
}

async fn store_background(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    statement: &str,
) -> Result<(), String> {
    let store = services
        .blackboard()
        .await
        .map_err(|error| error.to_string())?;
    let normalized = statement.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(normalized.as_bytes());
    let id = BlackboardEntryId::parse(format!("stateful-user-background-{:x}", hasher.finalize()))
        .map_err(|error| error.to_string())?;
    let (entry, outcome) = match store
        .get_entry(project_id, &id)
        .await
        .map_err(|error| error.to_string())?
    {
        Some(existing) if existing.state == BlackboardEntryState::Active => {
            (existing, CaptureOutcome::AlreadyStored)
        }
        Some(_) => return Ok(()),
        None => {
            let node_id = services.project_node_id(project_id).await?;
            let confidence = ConfidenceScore::from_basis_points(USER_RULE_CONFIDENCE_BASIS_POINTS)
                .map_err(|error| error.to_string())?;
            let entry = store
                .create_entry(
                    id,
                    NewBlackboardEntry {
                        project_id: project_id.to_string(),
                        node_id,
                        kind: BlackboardKind::Fact,
                        content: statement.to_string(),
                        structured_value: None,
                        confidence,
                        verification: BlackboardVerification::Unverified,
                        importance: BlackboardImportance::High,
                        root_promotion: RootPromotion::Promoted,
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
            (entry, CaptureOutcome::Stored)
        }
    };
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::KnowledgeCaptured {
            project_id: project_id.to_string(),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            entry_id: entry.id.to_string(),
            revision: entry.revision,
            category: KnowledgeCategory::Background,
            outcome,
            text: receipt_text(&entry.value.content),
        });
    }
    Ok(())
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
            RuleSource {
                thread_id,
                turn_id,
                receipt_turn_id: turn_id,
                // The message starting this turn follows every retirement recorded so far.
                stated_at_ms: i64::MAX,
            },
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

/// Where a rule came from and which turn's receipt reports it.
pub(crate) struct RuleSource<'a> {
    pub(crate) thread_id: &'a str,
    /// The turn whose user message holds the rule.
    pub(crate) turn_id: &'a str,
    /// The turn being run now, which the receipt belongs to.
    pub(crate) receipt_turn_id: &'a str,
    /// When the user wrote that message (Unix milliseconds).
    pub(crate) stated_at_ms: i64,
}

/// Stores one rule in the user's exact words (the whole clause they wrote), or returns the
/// entry already holding that wording.
pub(crate) async fn store_user_rule(
    services: &ProjectIntelligenceServices,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    source: RuleSource<'_>,
    clause: &str,
    standing: RuleStanding,
) -> Result<CapturedRule, String> {
    let RuleSource {
        thread_id,
        turn_id,
        receipt_turn_id,
        stated_at_ms,
    } = source;
    let store = services
        .blackboard()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = |entry: &BlackboardEntry, outcome: CaptureOutcome| {
        let category = if entry.value.root_promotion == RootPromotion::Promoted {
            KnowledgeCategory::Rule
        } else {
            KnowledgeCategory::PendingRule
        };
        if let Some(event_sink) = event_sink {
            event_sink.emit(StatefulEvent::KnowledgeCaptured {
                project_id: project_id.to_string(),
                thread_id: thread_id.to_string(),
                turn_id: receipt_turn_id.to_string(),
                entry_id: entry.id.to_string(),
                revision: entry.revision,
                category,
                outcome,
                text: receipt_text(&entry.value.content),
            });
        }
    };
    // The current generation of this wording, or the first free one after inactive history.
    let mut id = None;
    let mut retired_at_ms = None;
    for generation in 0..MAX_RULE_GENERATIONS {
        let candidate = user_rule_entry_id(project_id, clause, generation)
            .ok_or_else(|| "the rule cannot be identified".to_string())?;
        match store
            .get_entry(project_id, &candidate)
            .await
            .map_err(|error| error.to_string())?
        {
            None => {
                id = Some(candidate);
                break;
            }
            Some(existing) if existing.state == BlackboardEntryState::Active => {
                // Promoting a pending wording needs the same authority as re-establishing a
                // retired one: a message written after the retirement.
                if standing == RuleStanding::Standing
                    && existing.value.root_promotion != RootPromotion::Promoted
                    && retired_at_ms.is_some_and(|retired| stated_at_ms <= retired)
                {
                    return Err(RETIRED_BEFORE_MESSAGE.to_string());
                }
                return reconcile_active_rule(
                    store,
                    event_sink,
                    project_id,
                    user_message_source(thread_id, turn_id),
                    existing,
                    standing,
                    &receipt,
                )
                .await;
            }
            // Retired or superseded: the user restating it re-establishes it below.
            Some(inactive) => {
                retired_at_ms = retired_at_ms.max(Some(inactive.updated_at_ms));
            }
        }
    }
    // Only a message written after the retirement restores the rule; quoting the message
    // that first stated it (or any other earlier one) never does.
    if retired_at_ms.is_some_and(|retired| stated_at_ms <= retired) {
        return Err(RETIRED_BEFORE_MESSAGE.to_string());
    }
    let id = id.ok_or_else(|| {
        "this rule was retired too many times to be stored again automatically".to_string()
    })?;
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
    receipt(&entry, CaptureOutcome::Stored);
    Ok(CapturedRule {
        entry,
        standing,
        newly_stored: true,
    })
}

/// A pending rule restated as standing applies; a later task-limited mention never demotes a
/// current standing rule. Receipts report the stored state.
async fn reconcile_active_rule(
    store: &BlackboardStore,
    event_sink: Option<&dyn StatefulEventSink>,
    project_id: &str,
    source_id: String,
    existing: BlackboardEntry,
    standing: RuleStanding,
    receipt: &impl Fn(&BlackboardEntry, CaptureOutcome),
) -> Result<CapturedRule, String> {
    if standing != RuleStanding::Standing
        || existing.value.root_promotion == RootPromotion::Promoted
    {
        receipt(&existing, CaptureOutcome::AlreadyStored);
        return Ok(CapturedRule {
            entry: existing,
            standing,
            newly_stored: false,
        });
    }
    let promoted = store
        .update_entry(
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
        )
        .await
        .map_err(|error| error.to_string())?;
    if let Some(event_sink) = event_sink {
        event_sink.emit(StatefulEvent::BlackboardUpdated {
            project_id: project_id.to_string(),
            entity_kind: BlackboardEntityKind::Entry,
            entity_id: promoted.id.to_string(),
            revision: promoted.revision,
        });
    }
    receipt(&promoted, CaptureOutcome::Stored);
    Ok(CapturedRule {
        entry: promoted,
        standing,
        newly_stored: true,
    })
}

#[cfg(test)]
#[path = "rule_capture_tests.rs"]
mod tests;
