//! Seals original host user items, including steering; never concatenates text parts.
use crate::SelectedProject;
use crate::SelectedThread;
use crate::StatefulExtension;
use codex_extension_api::ExtensionData;
use codex_project_intelligence::SourceObservation;
use codex_project_intelligence::SourceSpan;
use codex_project_intelligence::SourceSpanRole;
use codex_protocol::items::UserMessageItem;
use codex_protocol::user_input::UserInput;

#[cfg(test)]
#[path = "capture_sources_tests.rs"]
mod tests;

#[derive(Clone)]
pub(crate) struct SourceTurn {
    pub(crate) turn_id: String,
}

#[derive(Clone, Default)]
pub(crate) struct SourceHandles {
    handles: Vec<codex_project_intelligence::SourceSeal>,
    // Once the bounded identity set fills, repeated delivery cannot be distinguished
    // from another distinct source without unbounded state. Disclose a lower bound.
    additional_handles: bool,
}

impl StatefulExtension {
    pub(crate) async fn observe_original_item(
        &self,
        thread_store: &ExtensionData,
        turn_store: &ExtensionData,
        message: &UserMessageItem,
    ) {
        let (Some(selected), Some(thread), Some(turn), Some(services)) = (
            thread_store.get::<SelectedProject>(),
            thread_store.get::<SelectedThread>(),
            turn_store.get::<SourceTurn>(),
            self.services.as_ref(),
        ) else {
            return;
        };
        let Ok(thread_id) = codex_protocol::ThreadId::from_string(&thread.thread_id) else {
            return;
        };
        let admission = match codex_state::ThreadProjectAdmission::acquire(
            services.sqlite(),
            thread_id,
            selected.project_id(),
        )
        .await
        {
            Ok(Some(admission)) => admission,
            _ => {
                tracing::warn!(
                    "original source unavailable: authoritative project binding missing"
                );
                return;
            }
        };
        let parts: Vec<_> = message
            .content
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if let UserInput::Text { text, .. } = item {
                    Some((index, text))
                } else {
                    None
                }
            })
            .take(/*n*/ 9)
            .collect();
        let total = parts
            .iter()
            .try_fold(0usize, |sum, (_, text)| sum.checked_add(text.len()));
        let store = match services.blackboard().await {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(%error, "original source observation unavailable");
                return;
            }
        };
        if parts.len() > 8 || total.is_none_or(|bytes| bytes > 65536) {
            let omitted = SourceObservation {
                project_id: selected.project_id().to_string(),
                authoritative_thread_id: thread.thread_id.clone(),
                binding_generation: admission.binding_generation(),
                original_event_id: message.id.clone(),
                turn_id: turn.turn_id.clone(),
                part_index: 0,
                source_revision: 1,
                complete_envelope: false,
                incomplete_reason: Some(
                    "inspection exceeds eight parts or 64 KiB; native history retained".to_string(),
                ),
                ordered_spans: Vec::new(),
            };
            if let Err(error) = store.record_source_omission(omitted).await {
                tracing::warn!(%error, "original source omission unavailable");
            }
            return;
        }
        for (part_index, text) in parts {
            if text.is_empty() {
                continue;
            }
            let complete = text.len() <= 16384;
            let observation = SourceObservation {
                project_id: selected.project_id().to_string(),
                authoritative_thread_id: thread.thread_id.clone(),
                binding_generation: admission.binding_generation(),
                original_event_id: message.id.clone(),
                turn_id: turn.turn_id.clone(),
                part_index: part_index as u32,
                source_revision: 1,
                complete_envelope: complete,
                incomplete_reason: (!complete).then(|| {
                    "complete envelope exceeds 16 KiB; exact factual source only".to_string()
                }),
                ordered_spans: if complete {
                    vec![SourceSpan {
                        start_byte: 0,
                        end_byte: text.len() as u32,
                        role: SourceSpanRole::Body,
                    }]
                } else {
                    Vec::new()
                },
            };
            match store.observe_source(observation, text).await {
                Ok(seal) => {
                    // Observation is durable first; admission can fail without losing it.
                    self.admit_declaration(services, &admission, &seal, text)
                        .await;
                    let mut handles = turn_store
                        .get::<SourceHandles>()
                        .map(|handles| (*handles).clone())
                        .unwrap_or_default();
                    if !handles
                        .handles
                        .iter()
                        .any(|old| old.exact_source_locator == seal.exact_source_locator)
                    {
                        if handles.handles.len() < 8 {
                            handles.handles.push(seal);
                        } else {
                            handles.additional_handles = true;
                        }
                        turn_store.insert(handles);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "original source observation refused; no admission")
                }
            }
        }
    }
}

/// Existing World State wrapper carries a <1,000-byte, append-only source handle item.
/// Omission is exact until the identity set fills, then an explicit lower bound.
/// This avoids overflow and double-counting redelivery without an unbounded set.
pub(crate) fn handle_section(
    handles: Option<&SourceHandles>,
) -> Option<(usize, codex_extension_api::WorldStateSectionContribution)> {
    use codex_extension_api::PreviousWorldStateSection;
    use codex_extension_api::RenderedWorldStateFragment;
    use codex_extension_api::WorldStateSectionContribution;
    use serde_json::json;
    let handles = handles?;
    let omitted = handles.handles.len() + usize::from(handles.additional_handles);
    let mut body = json!({"source":"sealed user-delivered bytes; not endorsement", "handles":[], "omitted":omitted,
        "omittedIsExact": !handles.additional_handles, "selection":"first eight distinct sources"});
    for seal in &handles.handles {
        body["handles"].as_array_mut()?.push(json!([
            seal.exact_source_locator,
            seal.digest,
            seal.observation.source_revision,
            seal.observation.part_index,
            seal.original_utf8_length
        ]));
        body["omitted"] = json!(omitted - body["handles"].as_array()?.len());
        if body.to_string().len() > 850 {
            body["handles"].as_array_mut()?.pop();
            body["omitted"] = json!(omitted - body["handles"].as_array()?.len());
            break;
        }
    }
    let snapshot = body.clone();
    let text = body.to_string();
    let bytes = text.len() + 49;
    Some((
        bytes,
        WorldStateSectionContribution::new("capture_sources", snapshot.clone(), move |previous| {
            match previous {
                PreviousWorldStateSection::Known(previous) if previous == &snapshot => None,
                PreviousWorldStateSection::Absent
                | PreviousWorldStateSection::Unknown
                | PreviousWorldStateSection::Known(_) => Some(RenderedWorldStateFragment::new(
                    "developer",
                    ("<capture_sources>", "</capture_sources>"),
                    text.clone(),
                )),
            }
        }),
    ))
}
