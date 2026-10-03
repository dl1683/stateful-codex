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
use codex_project_intelligence::CaptureSource;
use codex_project_intelligence::ScopeState;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::models::MessagePhase;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::StatefulEventSink;
use crate::answer_group::GroupKind;
use crate::answer_group::Placement;
use crate::answer_group::Planned;
use crate::answer_group::Source;
use crate::answer_group::commit_group;
use crate::answer_units::AnswerUnitKind;
use crate::answer_units::SourceText;
use crate::answer_units::answer_units;
use crate::services::ProjectIntelligenceServices;

/// Longest answer opening kept with each unit, so recall can match the unit's topic.
const MAX_OPENING_BYTES: usize = 300;

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
    // Without the thread's investigation or a capture position the units cannot be placed
    // truthfully; capture nothing rather than widen them to the whole project.
    let scope = match store.thread_scope(project_id, thread_id).await {
        Ok(Some(scope)) if scope.state == ScopeState::Open => Some((scope.scope_id, scope.title)),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to read the thread's investigation; answer not captured");
            return;
        }
    };
    let source_sequence = match store.allocate_source_sequence(project_id).await {
        Ok(sequence) => Some(sequence),
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to allocate a capture position; answer not captured");
            return;
        }
    };
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
            .map(|(_, note)| Planned::Omitted(note.clone()))
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

#[cfg(test)]
#[path = "conversation_capture_tests.rs"]
mod tests;
