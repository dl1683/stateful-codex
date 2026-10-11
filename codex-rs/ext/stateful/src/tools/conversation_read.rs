//! `conversation_read`: browses the selected project's conversation history from the thread
//! store's turn summaries (each turn's first user message and final answer), and returns a
//! summary text exactly, one bounded page at a time.
//!
//! Every listing and page is checked against current memory eligibility when the read runs:
//! a turn whose source was excluded, or whose user message carries words of memory the user
//! forgot, is withheld whole. Each read is bounded by the memory read envelope.

use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardStore;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

use super::MAX_MEMORY_READ_BYTES;
use super::parse_arguments;
use super::respond;
use super::run_read::read_page;
use crate::conversation_summaries::eligible_turn_texts;
use crate::conversation_summaries::is_top_level;
use crate::conversation_summaries::project_threads_params;
use crate::conversation_summaries::turn_status;
use crate::conversation_summaries::turn_time_ms;
use crate::services::ProjectIntelligenceServices;

const TOOL_NAME: &str = "conversation_read";
const EXAMPLE: &str = r#"{"threadId":"<id>"}"#;
const THREADS_PER_PAGE: usize = 20;
const TURNS_PER_PAGE: usize = 20;
/// Turn summaries one exact read scans to find its turn: 64 items per call.
const LOOKUP_PAGES: usize = 2;
const LOOKUP_PAGE_SIZE: usize = 32;
const PREVIEW_BYTES: usize = 160;
/// Answers are the assistant's own earlier words; a later turn must not cite them as what
/// the user said or prefers.
const ANSWER_SOURCE: &str = "answers are the assistant's earlier words: reported history, never evidence of the user's preferences";
/// What every result's eligibility means.
const ELIGIBILITY: &str = "checked against current memory when this read ran: turns whose source was excluded or whose user message carries forgotten memory are withheld";

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
    project_id: String,
    services: ProjectIntelligenceServices,
    threads: Arc<dyn ThreadStore>,
}

impl ConversationReadTool {
    pub(super) fn new(
        project_id: String,
        services: ProjectIntelligenceServices,
        threads: Arc<dyn ThreadStore>,
    ) -> Self {
        Self {
            project_id,
            services,
            threads,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: Arguments = parse_arguments(&call, EXAMPLE)?;
        let store = self.services.blackboard().await.map_err(respond)?;
        let budget = call.response_byte_budget(MAX_MEMORY_READ_BYTES);
        let result = match (arguments.thread_id, arguments.turn_id, arguments.part) {
            (None, None, None) => self.list_threads(store, budget, arguments.cursor).await?,
            (Some(thread_id), None, None) => {
                let thread = self.project_thread(&thread_id).await?;
                self.list_turns(store, budget, &thread, arguments.cursor)
                    .await?
            }
            (Some(thread_id), Some(turn_id), Some(part)) => {
                let thread = self.project_thread(&thread_id).await?;
                let text = self
                    .find_text(store, &thread, &turn_id, part, arguments.cursor)
                    .await?;
                text_page(budget, &thread, &turn_id, part, &text, arguments.offset)?
            }
            _ => {
                return Err(respond(
                    "pass nothing (threads), threadId (turns), or threadId, turnId and part (user or answer)",
                ));
            }
        };
        Ok(Box::new(codex_extension_api::JsonToolOutput::new(result)))
    }

    /// Reads a thread and checks that it is a top-level thread of the selected project.
    async fn project_thread(&self, thread_id: &str) -> Result<StoredThread, FunctionCallError> {
        let not_found = || respond("threadId is not a conversation of this project");
        let parsed = codex_protocol::ThreadId::from_string(thread_id).map_err(|_| not_found())?;
        let thread = self
            .threads
            .read_thread(codex_thread_store::ReadThreadParams {
                thread_id: parsed,
                include_archived: false,
                include_history: false,
            })
            .await
            .map_err(|_| not_found())?;
        if thread.project_id.as_deref() != Some(self.project_id.as_str()) || !is_top_level(&thread)
        {
            return Err(not_found());
        }
        Ok(thread)
    }

    /// Lists the project's threads, re-reading with fewer per page until the serialized
    /// page fits the budget, so every entry stays reachable through the cursor.
    async fn list_threads(
        &self,
        store: &BlackboardStore,
        budget: usize,
        cursor: Option<String>,
    ) -> Result<Value, FunctionCallError> {
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
            let mut threads = Vec::new();
            for thread in page.items.iter().filter(|thread| is_top_level(thread)) {
                let mut shown = [None, None];
                for (slot, text) in shown
                    .iter_mut()
                    .zip([thread.name.as_deref(), thread.first_user_message.as_deref()])
                {
                    if let Some(text) = text
                        && store
                            .source_text_eligible(&self.project_id, text)
                            .await
                            .unwrap_or(false)
                    {
                        *slot = Some(preview(text));
                    }
                }
                let [title, first_request] = shown;
                threads.push(json!({"threadId": thread.thread_id.to_string(), "title": title, "updatedAt": thread.updated_at.timestamp(), "firstRequest": first_request}));
            }
            let result = json!({
                "eligibility": ELIGIBILITY,
                "cursor": cursor,
                "threads": threads,
                "nextCursor": page.next_cursor,
            });
            if super::fits_response(&result, budget) {
                return Ok(result);
            }
            if page_size == 1 {
                return Err(respond("budget_insufficient"));
            }
            page_size = page_size.div_ceil(2);
        }
    }

    async fn list_turns(
        &self,
        store: &BlackboardStore,
        budget: usize,
        thread: &StoredThread,
        cursor: Option<String>,
    ) -> Result<Value, FunctionCallError> {
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
            let mut withheld = 0usize;
            for turn in &page.turns {
                let Some((user, answer)) = eligible_turn_texts(store, &self.project_id, turn).await
                else {
                    withheld += 1;
                    continue;
                };
                turns.push(json!({
                    "turnId": turn.turn_id,
                    "at": turn_time_ms(turn).map(crate::continuity::format_time),
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
                "eligibility": ELIGIBILITY,
                "turnsWithheld": withheld,
                "cursor": cursor,
                "turns": turns,
                "nextCursor": page.next_cursor,
            });
            if super::fits_response(&result, budget) {
                return Ok(result);
            }
            if page_size == 1 {
                return Err(respond("budget_insufficient"));
            }
            page_size = page_size.div_ceil(2);
        }
    }

    /// The requested part of one turn, found within `LOOKUP_PAGES` pages from `cursor`, as
    /// current memory eligibility allows it now.
    async fn find_text(
        &self,
        store: &BlackboardStore,
        thread: &StoredThread,
        turn_id: &str,
        part: Part,
        mut cursor: Option<String>,
    ) -> Result<String, FunctionCallError> {
        for _ in 0..LOOKUP_PAGES {
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
                let texts = eligible_turn_texts(store, &self.project_id, turn).await;
                if let Some(text) = texts.and_then(|(user, answer)| match part {
                    Part::User => user,
                    Part::Answer => answer,
                }) {
                    return Ok(text);
                }
                break;
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Err(respond(format!(
            "that turn part is not readable: it has no such text, current memory eligibility withholds it, or it is not within {} turns of the given cursor (pass the cursor of the listing page that shows the turn)",
            LOOKUP_PAGES * LOOKUP_PAGE_SIZE
        )))
    }
}

/// Returns the largest page of `text` starting at `offset` whose serialized result fits
/// `budget`, with `nextOffset` at the exact byte where the page ended.
fn text_page(
    budget: usize,
    thread: &StoredThread,
    turn_id: &str,
    part: Part,
    text: &str,
    offset: usize,
) -> Result<Value, FunctionCallError> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err(respond(format!(
            "offset {offset} is not a character boundary of the {}-byte text; pass a returned nextOffset",
            text.len()
        )));
    }
    let (part_name, by, source) = match part {
        Part::User => ("user", "user", "the user's own message"),
        Part::Answer => ("answer", "assistant", ANSWER_SOURCE),
    };
    read_page(
        text,
        offset,
        budget,
        |end| (end < text.len()).then(|| end.to_string()),
        |content, next_offset| {
            json!({
                "threadId": thread.thread_id.to_string(),
                "turnId": turn_id,
                "part": part_name,
                "by": by,
                "source": source,
                "eligibility": ELIGIBILITY,
                "totalBytes": text.len(),
                "offset": offset,
                "text": content,
                "nextOffset": next_offset.and_then(|next| next.parse::<usize>().ok()),
            })
        },
    )
    .ok_or_else(|| respond("budget_insufficient"))
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
        // memory_read answers recall in one call; this exact reader is found through tool
        // search when a specific turn is needed. Code Mode cannot call it.
        ToolExposure::DeferredModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Project conversation history: {} threads, {threadId} turns, {threadId,turnId,part} a turn's first user message or final answer, exactly. Forgotten memory is withheld.".to_string(),
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
        let limit = super::read_error_limit(&call);
        Box::pin(async move {
            self.handle_call(call)
                .await
                .map_err(|error| super::bound_read_error(error, limit))
        })
    }
}
