//! Reads of the thread store's per-turn summaries (each turn's first user message and
//! final answer) for the selected project's top-level threads.

use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadSource;
use codex_thread_store::ListThreadsParams;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredThreadItem;
use codex_thread_store::StoredTurn;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::StoredTurnStatus;
use codex_thread_store::ThreadSortKey;
use codex_thread_store::ThreadStore;
use serde_json::Value;

/// The project's unarchived threads, most recently updated first.
pub(super) fn project_threads_params(
    project_id: &str,
    page_size: usize,
    cursor: Option<String>,
) -> ListThreadsParams {
    ListThreadsParams {
        page_size,
        cursor,
        sort_key: ThreadSortKey::UpdatedAt,
        sort_direction: SortDirection::Desc,
        allowed_sources: Vec::new(),
        model_providers: Some(Vec::new()),
        cwd_filters: None,
        section: None,
        project_id: Some(Some(project_id.to_string())),
        archived: false,
        search_term: None,
        relation_filter: None,
        use_state_db_only: true,
    }
}

/// Whether a thread is the user's own conversation rather than a subagent, guardian review,
/// or memory worker thread. Listings may omit `thread_source`, so the session source alone
/// must already exclude internal and subagent threads.
pub(super) fn is_top_level(thread: &StoredThread) -> bool {
    !matches!(
        thread.source,
        SessionSource::SubAgent(_) | SessionSource::Internal(_)
    ) && matches!(thread.thread_source, None | Some(ThreadSource::User))
}

/// The first user message and the final answer among a turn's summary items.
pub(super) fn turn_texts(items: &[StoredThreadItem]) -> (Option<String>, Option<String>) {
    let mut user = None;
    let mut answer = None;
    for item in items {
        let Ok(value) = serde_json::from_slice::<Value>(&item.item_json) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("userMessage") if user.is_none() => {
                user = value
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|content| {
                        content
                            .iter()
                            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|text| !text.is_empty());
            }
            Some("agentMessage") => {
                answer = value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            _ => {}
        }
    }
    (user, answer)
}

/// Completion time, or the start time of an unfinished turn, in Unix milliseconds.
pub(super) fn turn_time_ms(turn: &StoredTurn) -> Option<i64> {
    turn.completed_at
        .or(turn.started_at)
        .map(|seconds| seconds.saturating_mul(1000))
}

pub(super) fn turn_status(turn: &StoredTurn) -> Option<&'static str> {
    match turn.status {
        StoredTurnStatus::Completed => None,
        StoredTurnStatus::Interrupted => Some("interrupted"),
        StoredTurnStatus::Failed => Some("failed"),
        StoredTurnStatus::InProgress => Some("in progress"),
    }
}

/// Turns scanned, in pages, when looking up one turn by its ID.
const TURN_LOOKUP_PAGES: usize = 40;
const TURN_LOOKUP_PAGE_SIZE: usize = 50;

/// Reads a thread and checks that it is an unarchived top-level thread of `project_id`.
pub(super) async fn project_thread(
    threads: &dyn ThreadStore,
    project_id: &str,
    thread_id: &str,
) -> Result<StoredThread, String> {
    let parsed = ThreadId::from_string(thread_id).map_err(|error| error.to_string())?;
    let thread = threads
        .read_thread(ReadThreadParams {
            thread_id: parsed,
            include_archived: false,
            include_history: false,
        })
        .await
        .map_err(|error| error.to_string())?;
    if thread.project_id.as_deref() != Some(project_id) || !is_top_level(&thread) {
        return Err(format!(
            "thread {thread_id} is not a conversation of this project"
        ));
    }
    Ok(thread)
}

/// The first user message of one turn of a top-level thread of `project_id`.
pub(super) async fn project_turn_user_text(
    threads: &dyn ThreadStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
) -> Result<String, String> {
    let thread = project_thread(threads, project_id, thread_id).await?;
    let mut cursor = None;
    for _ in 0..TURN_LOOKUP_PAGES {
        let page = threads
            .list_turns(ListTurnsParams {
                thread_id: thread.thread_id,
                include_archived: false,
                cursor: cursor.take(),
                page_size: TURN_LOOKUP_PAGE_SIZE,
                sort_direction: SortDirection::Desc,
                items_view: StoredTurnItemsView::Summary,
            })
            .await
            .map_err(|error| error.to_string())?;
        if let Some(turn) = page.turns.iter().find(|turn| turn.turn_id == turn_id) {
            return turn_texts(&turn.items)
                .0
                .ok_or_else(|| format!("turn {turn_id} has no user message"));
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Err(format!(
        "turn {turn_id} was not found in thread {thread_id}"
    ))
}
