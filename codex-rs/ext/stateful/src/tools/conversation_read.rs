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
use codex_protocol::ThreadId;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::ReadThreadParams;
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
/// Turn pages scanned to find one turn by ID (up to 2,000 turns of one thread).
const MAX_LOOKUP_PAGES: usize = 40;
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
    project_id: String,
    threads: Arc<dyn ThreadStore>,
}

impl ConversationReadTool {
    pub(super) fn new(project_id: String, threads: Arc<dyn ThreadStore>) -> Self {
        Self {
            project_id,
            threads,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
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
                self.read_text(&call, &thread, &turn_id, part, arguments.offset)
                    .await
            }
            (Some(_), None) => Err(respond("part (user or answer) is required with turnId")),
            (None, Some(_)) => Err(respond("turnId is required with part")),
        }
    }

    /// Reads a thread and checks that it is a top-level thread of the selected project.
    async fn project_thread(&self, thread_id: &str) -> Result<StoredThread, FunctionCallError> {
        let parsed = ThreadId::from_string(thread_id).map_err(respond)?;
        let thread = self
            .threads
            .read_thread(ReadThreadParams {
                thread_id: parsed,
                include_archived: false,
                include_history: false,
            })
            .await
            .map_err(respond)?;
        if thread.project_id.as_deref() != Some(self.project_id.as_str()) || !is_top_level(&thread)
        {
            return Err(respond(format!(
                "thread {thread_id} is not a conversation of this project"
            )));
        }
        Ok(thread)
    }

    async fn list_threads(
        &self,
        call: &ToolCall<'_>,
        cursor: Option<String>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let page = self
            .threads
            .list_threads(project_threads_params(
                &self.project_id,
                THREADS_PER_PAGE,
                cursor,
            ))
            .await
            .map_err(respond)?;
        let threads = page
            .items
            .iter()
            .filter(|thread| is_top_level(thread))
            .map(|thread| {
                json!({
                    "threadId": thread.thread_id.to_string(),
                    "title": thread.name,
                    "updatedAt": thread.updated_at.timestamp(),
                    "firstRequest": thread.first_user_message.as_deref().map(preview),
                })
            })
            .collect::<Vec<_>>();
        bounded_json_output(
            call,
            json!({ "threads": threads, "nextCursor": page.next_cursor }),
        )
    }

    async fn list_turns(
        &self,
        call: &ToolCall<'_>,
        thread: &StoredThread,
        cursor: Option<String>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let page = self
            .threads
            .list_turns(ListTurnsParams {
                thread_id: thread.thread_id,
                include_archived: false,
                cursor,
                page_size: TURNS_PER_PAGE,
                sort_direction: SortDirection::Desc,
                items_view: StoredTurnItemsView::Summary,
            })
            .await
            .map_err(respond)?;
        let turns = page
            .turns
            .iter()
            .map(|turn| {
                let (user, answer) = turn_texts(&turn.items);
                json!({
                    "turnId": turn.turn_id,
                    "atMs": turn_time_ms(turn),
                    "status": turn_status(turn).unwrap_or("completed"),
                    "user": user.as_deref().map(preview),
                    "userBytes": user.as_ref().map_or(0, String::len),
                    "answer": answer.as_deref().map(preview),
                    "answerBytes": answer.as_ref().map_or(0, String::len),
                })
            })
            .collect::<Vec<_>>();
        bounded_json_output(
            call,
            json!({
                "threadId": thread.thread_id.to_string(),
                "turns": turns,
                "nextCursor": page.next_cursor,
            }),
        )
    }

    async fn read_text(
        &self,
        call: &ToolCall<'_>,
        thread: &StoredThread,
        turn_id: &str,
        part: Part,
        offset: usize,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let mut cursor = None;
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
                let (user, answer) = turn_texts(&turn.items);
                let text = match part {
                    Part::User => user,
                    Part::Answer => answer,
                }
                .unwrap_or_default();
                return text_page(call, thread, turn_id, part, &text, offset);
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Err(respond(format!(
            "turn {turn_id} was not found among the newest {} turns of thread {}",
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
        ToolExposure::DirectModelOnly
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "This project's earlier conversation: {} lists threads, {threadId} its turns, {threadId, turnId, part} the exact text. Page with cursor and offset.".to_string(),
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
