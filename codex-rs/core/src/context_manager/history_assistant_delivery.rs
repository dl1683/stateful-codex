//! Resolves phase-less assistant deliveries once their response completes.
//!
//! A phase-less message is recorded `Pending`. At successful completion of its response, the last
//! such delivery becomes `Final` when nothing continues the response, and the others become
//! commentary. The resolution is an ordinary delivered-assistant event, so live recording and
//! rollout replay apply it identically, before any later compaction captures a packet. Only a
//! delivery whose finalized user-visible text is exactly its recorded text is resolved; hidden
//! markup or contributor rewrites leave it pending, so no packet claims the recorded provider text
//! is what the user saw. Resolution keeps the recorded source revision and delivery order.

use codex_history::AssistantDeliveryClassification;
use codex_history::RetainedContextEvent;
use codex_history::RetainedSource;
use codex_history::RetainedSourceRole;
use codex_history::RetainedUserMessage;
use codex_history::UserInputOrigin;
use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

use super::ContextManager;
use crate::guardian::GUARDIAN_MAX_ROOT_MESSAGE_TOKENS;
use crate::guardian::guardian_truncate_text;

/// A recorded phase-less assistant delivery awaiting its response's completion.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AssistantDeliveryCandidate {
    pub(crate) origin_thread_id: ThreadId,
    pub(crate) source: RetainedSource,
    /// Delivery order recorded with the original message.
    pub(crate) sequence: u64,
    /// Exact finalized text delivered to the user.
    pub(crate) text: String,
    pub(crate) recorded_text: RecordedText,
}

/// Whether the recorded message is exactly the finalized text the user saw.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordedText {
    Exact,
    /// Hidden markup was stripped or a contributor rewrote the delivered text.
    Differs,
}

/// How a successfully completed response left its turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResponseCompletion {
    /// No tool continuation and no `end_turn: false`: its last delivery answered the user.
    Answered,
    /// Tool calls or the provider continue the response, so its deliveries are commentary.
    Continues,
}

/// Classification events for a successfully completed response, in delivery order. Failed or
/// interrupted responses must not call this; their deliveries stay pending.
pub(crate) fn classify_completed_response(
    candidates: Vec<AssistantDeliveryCandidate>,
    completion: ResponseCompletion,
) -> Vec<RetainedContextEvent> {
    let last = candidates.len().checked_sub(1);
    candidates
        .into_iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.recorded_text == RecordedText::Exact)
        .map(|(index, candidate)| {
            let classification = match completion {
                ResponseCompletion::Answered if Some(index) == last => {
                    AssistantDeliveryClassification::Final
                }
                ResponseCompletion::Answered | ResponseCompletion::Continues => {
                    AssistantDeliveryClassification::Commentary
                }
            };
            // The same bounded excerpt and completeness as the original capture.
            let (text, _) =
                guardian_truncate_text(&candidate.text, GUARDIAN_MAX_ROOT_MESSAGE_TOKENS);
            RetainedContextEvent::DeliveredAssistantMessage {
                message: RetainedUserMessage {
                    turn_id: candidate.source.id.turn_id,
                    message_id: Some(candidate.source.id.message_id),
                    text,
                    complete: candidate.source.complete,
                    origin: UserInputOrigin::User,
                    phase: None,
                    origin_thread_id: Some(candidate.origin_thread_id),
                    classification: Some(classification),
                },
                acceptance_order: candidate.sequence,
            }
        })
        .collect()
}

impl ContextManager {
    /// The pending delivery recorded for `message_id`, if it is still in this history.
    pub(crate) fn assistant_delivery_candidate(
        &self,
        message_id: &str,
        text: String,
    ) -> Option<AssistantDeliveryCandidate> {
        let envelope = self.items.iter().rev().find(|envelope| {
            envelope
                .item
                .id()
                .is_some_and(|id| id.as_str() == message_id)
        })?;
        let metadata = envelope.metadata.as_ref()?;
        let ResponseItem::Message { content, .. } = &envelope.item else {
            return None;
        };
        if metadata.assistant_delivery_classification
            != Some(AssistantDeliveryClassification::Pending)
        {
            return None;
        }
        // A single text part is recorded verbatim; anything else cannot be compared exactly.
        let recorded_text = match content.as_slice() {
            [ContentItem::OutputText { text: recorded }] if *recorded == text => {
                RecordedText::Exact
            }
            _ => RecordedText::Differs,
        };
        Some(AssistantDeliveryCandidate {
            origin_thread_id: metadata.conversation_origin_thread_id?,
            source: metadata.retained_source.clone()?,
            sequence: metadata.user_input_order?,
            text,
            recorded_text,
        })
    }

    /// Applies a recorded resolution to its still-pending original envelope.
    pub(super) fn apply_assistant_delivery_classification(
        &mut self,
        message: &RetainedUserMessage,
    ) {
        let (Some(classification), Some(message_id)) =
            (message.classification, message.message_id.as_deref())
        else {
            return;
        };
        let items = std::sync::Arc::make_mut(&mut self.items);
        if let Some(metadata) = items
            .iter_mut()
            .rev()
            .filter_map(|envelope| envelope.metadata.as_mut())
            .find(|metadata| {
                metadata.retained_source.as_ref().is_some_and(|source| {
                    source.id.role == RetainedSourceRole::Assistant
                        && source.id.message_id == message_id
                        && source.id.turn_id == message.turn_id
                })
            })
            && metadata.assistant_delivery_classification
                == Some(AssistantDeliveryClassification::Pending)
        {
            metadata.assistant_delivery_classification = Some(classification);
        }
    }
}

#[cfg(test)]
#[path = "history_assistant_delivery_tests.rs"]
mod tests;
