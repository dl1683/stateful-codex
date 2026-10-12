//! Bounded reads of persisted thread history.
//!
//! Exec never hydrates a whole thread. Reconciling an Autonomous wait reads one summary page of
//! turns, and the completion backfill reads at most `TURN_ITEMS_MAX_PAGES` pages of
//! `TURN_ITEMS_PAGE_LIMIT` items of one turn, newest first. The bound covers requests, pages and
//! item counts only: payload bytes are not bounded, so a single large item is passed through whole.

use std::future::Future;

use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemsListCursor;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;

const RECONCILE_TURNS_LIMIT: u32 = 4;
const TURN_ITEMS_PAGE_LIMIT: u32 = 50;
const TURN_ITEMS_MAX_PAGES: usize = 2;

/// Finds the newest in-progress turn other than `current_turn_id` in one page of the thread's
/// newest summary turns.
pub(crate) async fn newest_other_in_progress_turn<F, Fut>(
    thread_id: &str,
    current_turn_id: &str,
    list_turns: F,
) -> Result<Option<Turn>, String>
where
    F: FnOnce(ThreadTurnsListParams) -> Fut,
    Fut: Future<Output = Result<ThreadTurnsListResponse, String>>,
{
    let page = list_turns(ThreadTurnsListParams {
        thread_id: thread_id.to_string(),
        cursor: None,
        limit: Some(RECONCILE_TURNS_LIMIT),
        sort_direction: Some(SortDirection::Desc),
        items_view: Some(TurnItemsView::Summary),
    })
    .await?;
    Ok(page
        .data
        .into_iter()
        .find(|turn| turn.id != current_turn_id && turn.status == TurnStatus::InProgress))
}

/// Returns the newest items of `turn_id` in turn order, reading at most `TURN_ITEMS_MAX_PAGES`
/// pages. Older items of a longer turn are not returned. A cursor that never advances costs at
/// most the same page budget.
pub(crate) async fn newest_turn_items<F, Fut>(
    thread_id: &str,
    turn_id: &str,
    mut list_items: F,
) -> Result<Vec<ThreadItem>, String>
where
    F: FnMut(ThreadItemsListParams) -> Fut,
    Fut: Future<Output = Result<ThreadItemsListResponse, String>>,
{
    let mut items = Vec::new();
    let mut cursor = None;
    for _ in 0..TURN_ITEMS_MAX_PAGES {
        let page = list_items(ThreadItemsListParams {
            thread_id: thread_id.to_string(),
            turn_id: Some(turn_id.to_string()),
            cursor: cursor.take().map(ThreadItemsListCursor::Opaque),
            limit: Some(TURN_ITEMS_PAGE_LIMIT),
            sort_direction: Some(SortDirection::Desc),
        })
        .await?;
        items.extend(page.data.into_iter().map(|entry| entry.item));
        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        cursor = Some(next_cursor);
    }
    items.reverse();
    Ok(items)
}

#[cfg(test)]
#[path = "history_backfill_tests.rs"]
mod tests;
