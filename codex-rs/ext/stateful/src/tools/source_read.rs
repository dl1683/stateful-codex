//! Exact original parts, including steering, rather than summaries or assistant quotations.
use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::respond;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolOutput;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::SourceSearchCursor;
use serde::Deserialize;
use serde_json::json;

#[cfg(test)]
#[path = "source_read_tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SourceRead {
    pub(super) source_id: String,
    pub(super) digest: String,
    pub(super) source_revision: u64,
    #[serde(default)]
    pub(super) offset: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SourceSearch {
    pub(super) source_query: String,
    pub(super) source_cursor: Option<SourceSearchCursor>,
}

pub(super) async fn search(
    store: &BlackboardStore,
    project: &str,
    arguments: SourceSearch,
    call: &ToolCall<'_>,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    if arguments.source_cursor.as_ref().is_some_and(|cursor| {
        serde_json::to_string(cursor).map_or(true, |cursor| cursor.len() > 1024)
    }) {
        return Err(respond(
            "source cursor exceeds 1024 bytes; restart discovery",
        ));
    }
    let page = store
        .search_source_ranges_with_budget(
            project,
            &arguments.source_query,
            arguments.source_cursor.as_ref(),
            call.response_byte_budget(MAX_RESPONSE_BYTES)
                .saturating_sub(200),
        )
        .await
        .map_err(respond)?;
    let mut result = serde_json::to_value(page).map_err(respond)?;
    result["label"] = json!(
        "source recall: original user-delivered parts, not endorsement; ranges are excerpts, read sourceId/digest/revision from seal for whole context"
    );
    bounded_json_output(call, result)
}

pub(super) async fn read(
    store: &BlackboardStore,
    project: &str,
    arguments: SourceRead,
    call: &ToolCall<'_>,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let page = store
        .read_source_page(
            project,
            &arguments.source_id,
            &arguments.digest,
            arguments.source_revision,
            arguments.offset,
        )
        .await
        .map_err(respond)?;
    let total = page.seal.original_utf8_length;
    let start = page.start_byte;
    let result = super::run_read::read_page(&page.exact_text, /*offset*/ 0,
        call.response_byte_budget(MAX_RESPONSE_BYTES), |end| (start + (end as u32) < total).then(|| (start + end as u32).to_string()),
        |text, next| json!({
            "sourceId":arguments.source_id,"digest":arguments.digest,"sourceRevision":arguments.source_revision,
            "offset":start,"text":text,"nextOffset":next.and_then(|offset| offset.parse::<u32>().ok()),
            "complete":start + text.len() as u32 == total,
            "label":"source recall: user-delivered bytes; speaker/endorsement/truth not certified",
        }),
    ).ok_or_else(|| respond("budget_insufficient: increase budget; no continuation issued"))?;
    bounded_json_output(call, result)
}
