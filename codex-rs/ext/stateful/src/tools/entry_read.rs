//! Exact, revision-bound text recovery when a root or query preview is capped.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolOutput;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardStore;
use serde_json::json;

use super::fits_response;

pub(super) async fn read(
    store: &BlackboardStore,
    project_id: &str,
    id: String,
    expected_revision: Option<u64>,
    offset: usize,
    budget: usize,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let id = BlackboardEntryId::parse(id)
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
    let entry = store
        .get_entry(project_id, &id)
        .await
        .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?
        .ok_or_else(|| FunctionCallError::RespondToModel("entry not found".to_string()))?;
    if expected_revision.is_some_and(|revision| revision != entry.revision)
        || (offset > 0 && expected_revision.is_none())
    {
        return Err(FunctionCallError::RespondToModel("entry revision changed or continuation has no expectedEntryRevision; restart at contentOffset=0".to_string()));
    }
    let words = &entry.value.content;
    if offset > words.len() || !words.is_char_boundary(offset) {
        return Err(FunctionCallError::RespondToModel(
            "contentOffset must be a UTF-8 boundary within the entry".to_string(),
        ));
    }
    let envelope = |end| {
        json!({
            "entryId": entry.id.to_string(), "revision": entry.revision,
            "state": entry.state, "source": entry.value.provenance,
            "contentOffset": offset, "content": &words[offset..end],
            "nextContentOffset": (end < words.len()).then_some(end),
            "complete": end == words.len(),
            "coverage": "exact stored words only; current applicability and verification are not asserted"
        })
    };
    let mut end = offset;
    for (index, ch) in words[offset..].char_indices().take(4096) {
        let next = offset + index + ch.len_utf8();
        if !fits_response(&envelope(next), budget) {
            break;
        }
        end = next;
    }
    if end == offset && offset < words.len() {
        return Err(FunctionCallError::RespondToModel(
            "budget_insufficient: increase the response budget; no continuation issued".to_string(),
        ));
    }
    let result = envelope(end);
    if !fits_response(&result, budget) {
        return Err(FunctionCallError::RespondToModel(
            "budget_insufficient: increase the response budget".to_string(),
        ));
    }
    Ok(Box::new(codex_extension_api::JsonToolOutput::new(result)))
}

#[cfg(test)]
#[path = "entry_read_tests.rs"]
mod tests;
