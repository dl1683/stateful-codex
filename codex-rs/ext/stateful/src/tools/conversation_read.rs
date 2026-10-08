//! `conversation_read`: browses the selected project's conversation history from the thread
//! store's turn summaries (each turn's first user message and final answer), and returns
//! any summary text exactly, one serialized-size-bounded page at a time.

use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use super::MAX_RESPONSE_BYTES;
use super::bounded_json_output;
use super::parse_arguments;
use super::respond;
use super::run_read::read_page;
use crate::conversation_summaries::is_top_level;
use crate::conversation_summaries::project_threads_params;
use crate::conversation_summaries::turn_status;
use crate::conversation_summaries::turn_texts;
use crate::conversation_summaries::turn_time_ms;

const TOOL_NAME: &str = "conversation_read";
const THREADS_PER_PAGE: usize = 20;
const TURNS_PER_PAGE: usize = 20;
/// Turn pages scanned to find one turn by ID from the given (or newest) position.
const MAX_LOOKUP_PAGES: usize = 40;
/// Answers are the assistant's own earlier words; a later turn must not cite them as what
/// the user said or prefers.
const ANSWER_SOURCE: &str = "answers are the assistant's earlier words: reported history, never evidence of the user's preferences";
const LOOKUP_PAGE_SIZE: usize = 50;
const PREVIEW_BYTES: usize = 160;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Part {
    User,
    Answer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    thread_id: Option<String>,
    turn_id: Option<String>,
    part: Option<Part>,
    #[serde(default)]
    offset: usize,
    cursor: Option<String>,
}

pub(super) struct ConversationReadTool {
    services: crate::services::ProjectIntelligenceServices,
    project_id: String,
    threads: Arc<dyn ThreadStore>,
}

impl ConversationReadTool {
    pub(super) fn new(
        project_id: String,
        threads: Arc<dyn ThreadStore>,
        services: crate::services::ProjectIntelligenceServices,
    ) -> Self {
        Self {
            services,
            project_id,
            threads,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        if call.function_arguments()?.len() > 32768 {
            return Err(respond(
                "source/history read exceeds the 32 KiB input bound",
            ));
        }
        let arguments: Arguments = parse_arguments(&call)?;
        let Some(thread_id) = arguments.thread_id else {
            if arguments.turn_id.is_some() || arguments.part.is_some() {
                return Err(respond("turnId and part require threadId"));
            }
            return self.list_threads(&call, arguments.cursor).await;
        };
        let thread = self.project_thread(&thread_id).await?;
        match (arguments.turn_id, arguments.part) {
            (None, None) => self.list_turns(&call, &thread, arguments.cursor).await,
            (Some(turn_id), Some(part)) => {
                self.read_text(
                    &call,
                    &thread,
                    &turn_id,
                    part,
                    arguments.offset,
                    arguments.cursor,
                )
                .await
            }
            (Some(_), None) => Err(respond("part (user or answer) is required with turnId")),
            (None, Some(_)) => Err(respond("turnId is required with part")),
        }
    }

    /// Reads a thread and checks that it is a top-level thread of the selected project.
    async fn project_thread(&self, thread_id: &str) -> Result<StoredThread, FunctionCallError> {
        crate::conversation_summaries::project_thread(
            self.threads.as_ref(),
            &self.project_id,
            thread_id,
        )
        .await
        .map_err(respond)
    }

    /// Lists the project's threads, re-reading with fewer per page until the serialized
    /// page fits the result budget, so every entry stays reachable through the cursor.
    async fn list_threads(
        &self,
        call: &ToolCall<'_>,
        cursor: Option<String>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let mut page_size = THREADS_PER_PAGE;
        loop {
            let page = self
                .threads
                .list_threads(project_threads_params(
                    &self.project_id,
                    page_size,
                    cursor.clone(),
                ))
                .await
                .map_err(respond)?;
            let store = self.services.blackboard().await.map_err(respond)?;
            let mut threads = Vec::new();
            for thread in page.items.iter().filter(|thread| is_top_level(thread)) {
                let mut title = None;
                if let Some(text) = thread.name.as_deref()
                    && store
                        .source_text_eligible(&self.project_id, text)
                        .await
                        .map_err(respond)?
                {
                    title = Some(preview(text));
                }
                let mut first_request = None;
                if let Some(text) = thread.first_user_message.as_deref()
                    && store
                        .source_text_eligible(&self.project_id, text)
                        .await
                        .map_err(respond)?
                {
                    first_request = Some(preview(text));
                }
                threads.push(json!({"threadId": thread.thread_id.to_string(), "title": title, "updatedAt": thread.updated_at.timestamp(), "firstRequest": first_request}));
            }
            let result = json!({
                "cursor": cursor,
                "threads": threads,
                "nextCursor": page.next_cursor,
            });
            if result.to_string().len() <= budget || page_size == 1 {
                return bounded_json_output(call, result);
            }
            page_size = page_size.div_ceil(2);
        }
    }

    async fn list_turns(
        &self,
        call: &ToolCall<'_>,
        thread: &StoredThread,
        cursor: Option<String>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let mut page_size = TURNS_PER_PAGE;
        loop {
            let page = self
                .threads
                .list_turns(ListTurnsParams {
                    thread_id: thread.thread_id,
                    include_archived: false,
                    cursor: cursor.clone(),
                    page_size,
                    sort_direction: SortDirection::Desc,
                    items_view: StoredTurnItemsView::Summary,
                })
                .await
                .map_err(respond)?;
            let mut turns = Vec::new();
            for turn in &page.turns {
                let store = self.services.blackboard().await.map_err(respond)?;
                if !store
                    .source_turn_eligible(&self.project_id, &turn.turn_id)
                    .await
                    .map_err(respond)?
                {
                    continue;
                }
                let (user, answer) = turn_texts(&turn.items);
                let user = match user {
                    Some(text)
                        if store
                            .source_text_eligible(&self.project_id, &text)
                            .await
                            .map_err(respond)? =>
                    {
                        Some(text)
                    }
                    _ => None,
                };
                let answer = match answer {
                    Some(text)
                        if store
                            .source_text_eligible(&self.project_id, &text)
                            .await
                            .map_err(respond)? =>
                    {
                        Some(text)
                    }
                    _ => None,
                };
                turns.push(json!({
                    "turnId": turn.turn_id,
                    "atMs": turn_time_ms(turn),
                    "status": turn_status(turn).unwrap_or("completed"),
                    "user": user.as_deref().map(preview),
                    "userBytes": user.as_ref().map_or(0, String::len),
                    "answer": answer.as_deref().map(preview),
                    "answerBytes": answer.as_ref().map_or(0, String::len),
                }));
            }
            // `cursor` is echoed so an exact read can start from the page that lists a turn.
            let result = json!({
                "threadId": thread.thread_id.to_string(),
                "answerSource": ANSWER_SOURCE,
                "cursor": cursor,
                "turns": turns,
                "nextCursor": page.next_cursor,
            });
            if result.to_string().len() <= budget || page_size == 1 {
                return bounded_json_output(call, result);
            }
            page_size = page_size.div_ceil(2);
        }
    }

    async fn read_text(
        &self,
        call: &ToolCall<'_>,
        thread: &StoredThread,
        turn_id: &str,
        part: Part,
        offset: usize,
        mut cursor: Option<String>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        for _ in 0..MAX_LOOKUP_PAGES {
            let page = self
                .threads
                .list_turns(ListTurnsParams {
                    thread_id: thread.thread_id,
                    include_archived: false,
                    cursor: cursor.take(),
                    page_size: LOOKUP_PAGE_SIZE,
                    sort_direction: SortDirection::Desc,
                    items_view: StoredTurnItemsView::Summary,
                })
                .await
                .map_err(respond)?;
            if let Some(turn) = page.turns.iter().find(|turn| turn.turn_id == turn_id) {
                if !self
                    .services
                    .blackboard()
                    .await
                    .map_err(respond)?
                    .source_turn_eligible(&self.project_id, turn_id)
                    .await
                    .map_err(respond)?
                {
                    return Err(respond("original turn fallback excluded after retirement"));
                }
                let (user, answer) = turn_texts(&turn.items);
                let text = match part {
                    Part::User => user,
                    Part::Answer => answer,
                }
                .unwrap_or_default();
                if !self
                    .services
                    .blackboard()
                    .await
                    .map_err(respond)?
                    .source_text_eligible(&self.project_id, &text)
                    .await
                    .map_err(respond)?
                {
                    return Err(respond(
                        "source unavailable for automatic recall after retirement; original archival history remains stored",
                    ));
                }
                return text_page(call, thread, turn_id, part, &text, offset);
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Err(respond(format!(
            "turn {turn_id} was not found within {} turns of thread {} from the given cursor; pass the cursor of the listing page that shows the turn",
            MAX_LOOKUP_PAGES * LOOKUP_PAGE_SIZE,
            thread.thread_id
        )))
    }
}

/// Returns the largest page of `text` starting at `offset` whose serialized result fits
/// the call's budget, with `nextOffset` at the exact byte where the page ended.
fn text_page(
    call: &ToolCall<'_>,
    thread: &StoredThread,
    turn_id: &str,
    part: Part,
    text: &str,
    offset: usize,
) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err(respond(format!(
            "offset {offset} is not a character boundary of the {}-byte text; pass a returned nextOffset",
            text.len()
        )));
    }
    let page = read_page(
        text,
        offset,
        call.response_byte_budget(MAX_RESPONSE_BYTES),
        |end| (end < text.len()).then(|| end.to_string()),
        |content, next_offset| {
            json!({
                "threadId": thread.thread_id.to_string(),
                "turnId": turn_id,
                "part": match part {
                    Part::User => "user",
                    Part::Answer => "answer",
                },
                "source": match part {
                    Part::User => "the user's own message",
                    Part::Answer => ANSWER_SOURCE,
                },
                "totalBytes": text.len(),
                "offset": offset,
                "text": content,
                "nextOffset": next_offset.and_then(|next| next.parse::<usize>().ok()),
            })
        },
    )
    .ok_or_else(|| respond("the result budget cannot hold any text"))?;
    Ok(Box::new(codex_extension_api::JsonToolOutput::new(page)))
}

fn preview(text: &str) -> String {
    let mut end = text.len().min(PREVIEW_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if end < text.len() {
        format!("{}…", &text[..end])
    } else {
        text.to_string()
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for ConversationReadTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn exposure(&self) -> ToolExposure {
        // memory_read answers recall in one call; this full-transcript reader is found
        // through tool search when a specific turn is needed.
        ToolExposure::DeferredModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Project history: {} threads, {threadId} turns, {threadId,turnId,part} summary. Original-source search and exact-source recall are unavailable. Source recall is not standing rules.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "threadId": {"type": "string"},
                    "turnId": {"type": "string"},
                    "part": {"type": "string", "enum": ["user", "answer"]},
                    "offset": {"type": "integer", "minimum": 0},
                    "cursor": {"type": "string"}
                },
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static conversation read schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}
