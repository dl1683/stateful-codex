//! Gathers the continuity record from the thread store's per-turn summaries (first user
//! message and final answer) of the selected project's recent top-level threads.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredTurn;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::ThreadStore;

use crate::continuity::CapturedTurn;
use crate::continuity::ContinuityRecord;
use crate::conversation_summaries::is_top_level;
use crate::conversation_summaries::project_threads_params;
use crate::conversation_summaries::turn_status;
use crate::conversation_summaries::turn_texts;
use crate::conversation_summaries::turn_time_ms;

const MAX_THREADS: usize = 5;
/// Threads listed to find `MAX_THREADS` top-level ones among subagent threads.
const MAX_THREADS_LISTED: usize = 25;
const MAX_TURNS_PER_THREAD: usize = 8;
const MAX_TURNS: usize = 10;

/// Reads the newest turn summaries of the project's top-level, unarchived threads.
pub(super) async fn gather_continuity(
    threads: &dyn ThreadStore,
    project_id: &str,
    current_thread_id: &str,
) -> ContinuityRecord {
    let mut more_turns = false;
    let mut unreadable_threads = 0;
    let mut history_unavailable = false;
    let mut turns = Vec::new();
    let listed = match threads
        .list_threads(project_threads_params(
            project_id,
            MAX_THREADS_LISTED,
            /*cursor*/ None,
        ))
        .await
    {
        Ok(page) => {
            more_turns |= page.next_cursor.is_some();
            page.items
        }
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to list project threads for continuity");
            history_unavailable = true;
            Vec::new()
        }
    };
    let top_level = listed.into_iter().filter(is_top_level).collect::<Vec<_>>();
    more_turns |= top_level.len() > MAX_THREADS;
    for thread in top_level.iter().take(MAX_THREADS) {
        let page = match threads
            .list_turns(ListTurnsParams {
                thread_id: thread.thread_id,
                include_archived: false,
                cursor: None,
                page_size: MAX_TURNS_PER_THREAD,
                sort_direction: SortDirection::Desc,
                items_view: StoredTurnItemsView::Summary,
            })
            .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::debug!(thread_id = %thread.thread_id, %error, "thread has no readable turn summaries");
                unreadable_threads += 1;
                continue;
            }
        };
        more_turns |= page.next_cursor.is_some();
        let current_thread = thread.thread_id.to_string() == current_thread_id;
        turns.extend(
            page.turns
                .into_iter()
                .filter_map(|turn| captured_turn(thread, current_thread, turn)),
        );
    }
    turns.sort_by_key(|turn| std::cmp::Reverse(turn.at_ms));
    if turns.len() > MAX_TURNS {
        turns.truncate(MAX_TURNS);
        more_turns = true;
    }
    ContinuityRecord {
        project_id: project_id.to_string(),
        captured_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            }),
        turns,
        more_turns,
        unreadable_threads,
        history_unavailable,
    }
}

fn captured_turn(
    thread: &StoredThread,
    current_thread: bool,
    turn: StoredTurn,
) -> Option<CapturedTurn> {
    let (user, answer) = turn_texts(&turn.items);
    if user.is_none() && answer.is_none() {
        return None;
    }
    Some(CapturedTurn {
        thread_id: thread.thread_id.to_string(),
        thread_title: thread
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        current_thread,
        at_ms: turn_time_ms(&turn),
        unfinished_status: turn_status(&turn),
        turn_id: turn.turn_id,
        user,
        answer,
    })
}
