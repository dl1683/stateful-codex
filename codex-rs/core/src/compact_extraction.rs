//! Contributor extraction inside the native LOCAL compaction request.
//!
//! With no registered `CompactionExtractionContributor` nothing here runs and the request,
//! output recording and summary are unchanged. Otherwise each native attempt freezes its final
//! request with host origin mappings, the contributor may append one bounded instruction, the
//! attempt's output is buffered until completion, and the trailer after the native summary is
//! separated before any output reaches history.

use std::collections::HashMap;
use std::sync::Arc;

use codex_extension_api::CompactionExtractionContributor;
use codex_extension_api::CompactionExtractionPreparation;
use codex_extension_api::CompactionExtractionResult;
use codex_extension_api::CompactionRequestSnapshot;
use codex_extension_api::CompactionSourceClass;
use codex_extension_api::CompactionSourceOrigin;
use codex_extension_api::CompactionSourcePart;
use codex_extension_api::CompactionTrailer;
use codex_extension_api::MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES;
use codex_extension_api::MAX_COMPACTION_EXTRACTION_TRAILER_BYTES;
use codex_history::AssistantDeliveryClassification;
use codex_history::ResponseItemEnvelope;
use codex_history::RetainedSourceRole;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::ResponseItem;

use crate::compact::is_summary_message;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::stream_events_utils::last_assistant_message_from_item;
use crate::stream_events_utils::raw_assistant_output_text_from_item;

/// One attempt's frozen request and the contributor addition it carries.
pub(crate) struct CompactionExtraction {
    contributor: Arc<dyn CompactionExtractionContributor>,
    request: Arc<CompactionRequestSnapshot>,
    preparation: CompactionExtractionPreparation,
}

impl CompactionExtraction {
    /// Freezes `request` (the attempt's final normalized input, whose last item is the native
    /// compaction instruction) and asks contributors, in registration order, for one addition.
    /// Withheld for Responses Lite, whose request prefix shifts wire positions.
    pub(crate) fn prepare(
        sess: &Session,
        turn_context: &TurnContext,
        attempt: u32,
        recorded: &[ResponseItemEnvelope],
        request: &[ResponseItemEnvelope],
    ) -> Option<Self> {
        let contributors = sess
            .services
            .extensions
            .compaction_extraction_contributors();
        let model_info = turn_context.model_info();
        if contributors.is_empty() || model_info.use_responses_lite {
            return None;
        }
        // Mirrors the request builder: these items never reach the wire.
        let configuration_updates = if sess
            .services
            .model_client
            .reasoning_effort_override_enabled(model_info)
        {
            ConfigurationUpdates::Sent
        } else {
            ConfigurationUpdates::Filtered
        };
        let snapshot = Arc::new(snapshot_request(
            sess.thread_id,
            &turn_context.sub_id,
            attempt,
            recorded,
            request,
            configuration_updates,
        ));
        contributors.iter().find_map(|contributor| {
            let preparation =
                contributor.prepare(&snapshot, &sess.services.thread_extension_data)?;
            admissible(&preparation, &snapshot).then(|| Self {
                contributor: Arc::clone(contributor),
                request: Arc::clone(&snapshot),
                preparation,
            })
        })
    }

    /// The bounded instruction, sent as its own item after the native compaction instruction.
    pub(crate) fn instruction_item(&self) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: self.preparation.instruction.clone(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
    }

    /// Removes the trailer from the buffered output so only the native summary is recorded.
    pub(crate) fn separate_trailer(
        &self,
        output: &mut [ResponseItem],
    ) -> CodexResult<CompactionTrailer> {
        separate_trailer(output, &self.preparation.trailer_opening)
    }

    pub(crate) fn complete(self, sess: &Session, trailer: CompactionTrailer, native_attempts: u32) {
        self.contributor.complete(
            CompactionExtractionResult {
                request: self.request,
                preparation: self.preparation,
                native_attempts,
                trailer,
            },
            &sess.services.thread_extension_data,
        );
    }
}

fn admissible(
    preparation: &CompactionExtractionPreparation,
    request: &CompactionRequestSnapshot,
) -> bool {
    let opening = preparation.trailer_opening.as_str();
    !preparation.instruction.is_empty()
        && preparation.instruction.len() <= MAX_COMPACTION_EXTRACTION_INSTRUCTION_BYTES
        && !opening.trim().is_empty()
        && !opening.contains(['\n', '\r'])
        && request
            .parts
            .iter()
            .all(|part| !part.text.contains(opening))
}

/// Whether the request builder drops configuration-update items from the wire input.
#[derive(Clone, Copy)]
enum ConfigurationUpdates {
    Sent,
    Filtered,
}

fn snapshot_request(
    thread_id: codex_protocol::ThreadId,
    turn_id: &str,
    attempt: u32,
    recorded: &[ResponseItemEnvelope],
    request: &[ResponseItemEnvelope],
    configuration_updates: ConfigurationUpdates,
) -> CompactionRequestSnapshot {
    let recorded_by_revision = recorded
        .iter()
        .filter_map(|envelope| {
            let source = envelope.metadata.as_ref()?.retained_source.as_ref()?;
            Some((source.revision.as_str(), &envelope.item))
        })
        .collect::<HashMap<_, _>>();
    let mut parts = Vec::new();
    let mut wire_index = 0;
    for (position, envelope) in request.iter().enumerate() {
        if matches!(configuration_updates, ConfigurationUpdates::Filtered)
            && matches!(envelope.item, ResponseItem::ConfigurationUpdate { .. })
        {
            continue;
        }
        let item_index = wire_index;
        wire_index += 1;
        // The final item is the native compaction instruction, never a source.
        let class = if position + 1 == request.len() {
            CompactionSourceClass::HostContext
        } else {
            source_class(envelope)
        };
        match &envelope.item {
            ResponseItem::Message { content, .. } => {
                for (part_index, part) in content.iter().enumerate() {
                    let (ContentItem::InputText { text } | ContentItem::OutputText { text }) = part
                    else {
                        continue;
                    };
                    parts.push(CompactionSourcePart {
                        item_index,
                        part_index,
                        class,
                        text: text.clone(),
                        origin: source_origin(
                            envelope,
                            class,
                            part_index,
                            text,
                            &recorded_by_revision,
                        ),
                    });
                }
            }
            ResponseItem::FunctionCallOutput { output, .. }
            | ResponseItem::CustomToolCallOutput { output, .. } => {
                // Command outputs have no durable per-invocation identity here yet.
                if let FunctionCallOutputBody::Text(text) = &output.body {
                    parts.push(CompactionSourcePart {
                        item_index,
                        part_index: 0,
                        class,
                        text: text.clone(),
                        origin: None,
                    });
                }
            }
            _ => {}
        }
    }
    CompactionRequestSnapshot {
        thread_id,
        turn_id: turn_id.to_string(),
        attempt,
        sequence_watermark: recorded
            .iter()
            .filter_map(|envelope| envelope.metadata.as_ref()?.user_input_order)
            .max(),
        parts,
    }
}

fn source_class(envelope: &ResponseItemEnvelope) -> CompactionSourceClass {
    let metadata = envelope.metadata.as_ref();
    if metadata.is_some_and(|metadata| metadata.compaction_output) {
        return CompactionSourceClass::CompactionOutput;
    }
    if metadata.is_some_and(|metadata| metadata.inherited_user_message) {
        return CompactionSourceClass::Inherited;
    }
    match &envelope.item {
        ResponseItem::Message { role, .. } if role == "assistant" => {
            CompactionSourceClass::Assistant
        }
        ResponseItem::Message { role, .. } if role == "user" => {
            match crate::event_mapping::parse_turn_item(&envelope.item) {
                Some(TurnItem::UserMessage(user)) if is_summary_message(&user.message()) => {
                    CompactionSourceClass::CompactionOutput
                }
                Some(TurnItem::UserMessage(_)) => CompactionSourceClass::User,
                _ => CompactionSourceClass::HostContext,
            }
        }
        ResponseItem::FunctionCallOutput { .. } | ResponseItem::CustomToolCallOutput { .. } => {
            CompactionSourceClass::CommandOutput
        }
        _ => CompactionSourceClass::HostContext,
    }
}

/// Maps a sent part to its recorded original; any disagreement yields no mapping.
fn source_origin(
    envelope: &ResponseItemEnvelope,
    class: CompactionSourceClass,
    part_index: usize,
    text: &str,
    recorded_by_revision: &HashMap<&str, &ResponseItem>,
) -> Option<CompactionSourceOrigin> {
    let metadata = envelope.metadata.as_ref()?;
    let source = metadata.retained_source.as_ref()?;
    let role_matches = matches!(
        (class, source.id.role),
        (CompactionSourceClass::User, RetainedSourceRole::User)
            | (
                CompactionSourceClass::Assistant,
                RetainedSourceRole::Assistant
            )
    );
    // Recorded assistant bytes prove nothing about what the user saw unless delivery resolution
    // established that the finalized visible text equals them.
    let published = class != CompactionSourceClass::Assistant
        || matches!(
            metadata.assistant_delivery_classification,
            Some(
                AssistantDeliveryClassification::Final
                    | AssistantDeliveryClassification::Commentary
            )
        );
    if !role_matches || !published {
        return None;
    }
    let recorded_text = recorded_by_revision
        .get(source.revision.as_str())
        .and_then(|item| match item {
            ResponseItem::Message { content, .. } => match content.get(part_index)? {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                    Some(text.as_str())
                }
                ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => None,
            },
            _ => None,
        });
    Some(CompactionSourceOrigin {
        item_id: source.id.message_id.clone(),
        turn_id: source.id.turn_id.clone(),
        origin_thread_id: metadata.conversation_origin_thread_id,
        sequence: metadata.user_input_order,
        source_revision: source.revision.to_string(),
        byte_identical: source.complete && recorded_text == Some(text),
    })
}

/// Cuts the trailer out of the final assistant message at any occurrence of the request-scoped
/// opening, using the concatenated text native summary extraction reads, so an opening split
/// across parts is still found. Everything from the opening on is removed whether or not its
/// framing is valid; the validator judges the framing. Several openings, an opening outside the
/// final assistant message, or no visible summary before it fail the attempt.
pub(crate) fn separate_trailer(
    output: &mut [ResponseItem],
    opening: &str,
) -> CodexResult<CompactionTrailer> {
    let failure = || CodexErr::Stream("compaction output has no separable summary".to_string());
    let mut found = None;
    let mut last_assistant = None;
    for (item_index, item) in output.iter().enumerate() {
        let Some(text) = raw_assistant_output_text_from_item(item) else {
            continue;
        };
        last_assistant = Some(item_index);
        let mut positions = text.match_indices(opening).map(|(position, _)| position);
        if let Some(position) = positions.next() {
            if found.is_some() || positions.next().is_some() {
                return Err(failure());
            }
            found = Some((item_index, position));
        }
    }
    let Some((item_index, position)) = found else {
        return Ok(CompactionTrailer::Absent);
    };
    if last_assistant != Some(item_index) {
        return Err(failure());
    }
    let ResponseItem::Message { content, .. } = &mut output[item_index] else {
        return Err(failure());
    };
    // Map the concatenated offset back to the part holding it; drop every later part.
    let mut trailer = String::new();
    let mut offset = 0;
    let mut cut = None;
    for (part_index, part) in content.iter_mut().enumerate() {
        let ContentItem::OutputText { text } = part else {
            continue;
        };
        if cut.is_some() {
            trailer.push_str(text);
        } else if offset + text.len() > position {
            trailer = text.split_off(position - offset);
            cut = Some(part_index);
        } else {
            offset += text.len();
        }
    }
    let cut = cut.ok_or_else(failure)?;
    content.truncate(cut + 1);
    if let Some(ContentItem::OutputText { text }) = content.last_mut() {
        text.truncate(text.trim_end().len());
    }
    if last_assistant_message_from_item(&output[item_index], /*plan_mode*/ false).is_none() {
        return Err(failure());
    }
    Ok(if trailer.len() > MAX_COMPACTION_EXTRACTION_TRAILER_BYTES {
        CompactionTrailer::Oversize
    } else {
        CompactionTrailer::Present(trailer)
    })
}

#[cfg(test)]
#[path = "compact_extraction_tests.rs"]
mod tests;
