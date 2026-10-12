//! Bounded reads of persisted thread history: exec never hydrates a whole thread. Requests, pages
//! and item counts are bounded; payload bytes are not, so a large item is passed through whole.

use std::future::Future;

use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadItemsListCursor;
use codex_app_server_protocol::ThreadItemsListParams;
use codex_app_server_protocol::ThreadItemsListResponse;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStartedNotification;
use codex_app_server_protocol::TurnStatus;
use tracing::warn;

use crate::AutonomousWaitState;

const RECONCILE_TURNS_LIMIT: u32 = 4;
const TURN_ITEMS_PAGE_LIMIT: u32 = 50;
const TURN_ITEMS_MAX_PAGES: usize = 2;

/// Reconciles an Autonomous wait from one summary page of turns, otherwise from the run state.
pub(crate) async fn reconcile_autonomous_wait<T, TFut, R, RFut>(
    thread_id: &str,
    current_turn_id: &str,
    list_turns: T,
    read_run: R,
) -> Result<AutonomousWaitState, String>
where
    T: FnOnce(ThreadTurnsListParams) -> TFut,
    TFut: Future<Output = Result<ThreadTurnsListResponse, String>>,
    R: FnOnce() -> RFut,
    RFut: Future<Output = Result<Option<StatefulRun>, String>>,
{
    let page = list_turns(ThreadTurnsListParams {
        thread_id: thread_id.to_string(),
        cursor: None,
        limit: Some(RECONCILE_TURNS_LIMIT),
        sort_direction: Some(SortDirection::Desc),
        items_view: Some(TurnItemsView::Summary),
        persist_before_list: true,
    })
    .await?;
    if let Some(turn) = page
        .data
        .into_iter()
        .find(|turn| turn.id != current_turn_id && turn.status == TurnStatus::InProgress)
    {
        return Ok(AutonomousWaitState::TurnStarted(Box::new(
            TurnStartedNotification {
                thread_id: thread_id.to_string(),
                turn,
            },
        )));
    }
    Ok(match read_run().await? {
        Some(run)
            if run.mode == StatefulWorkflowMode::Autonomous
                && run.status == StatefulRunStatus::Running =>
        {
            AutonomousWaitState::Running
        }
        Some(run) => AutonomousWaitState::Terminal(run.status),
        None => AutonomousWaitState::Missing,
    })
}

/// The newest items of `turn_id` in turn order, from at most `TURN_ITEMS_MAX_PAGES` pages.
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

/// Fills a completed turn from a bounded backfill only when the completion carries no agent
/// message: a delivered final answer in the completion is authoritative. A backfill without an
/// agent message never fills the turn, so a plan cannot stand in for the final message.
pub(crate) fn apply_turn_backfill(turn: &mut Turn, backfill: Result<Vec<ThreadItem>, String>) {
    let turn_id = turn.id.as_str();
    let is_agent_message = |item: &ThreadItem| matches!(item, ThreadItem::AgentMessage { .. });
    match backfill {
        Ok(_) if turn.items.iter().any(is_agent_message) => {}
        Ok(items) if items.iter().any(is_agent_message) => turn.items = items,
        Ok(_) => warn!(
            %turn_id,
            "no final agent message among the newest backfilled turn items; keeping the delivered items"
        ),
        Err(err) => warn!(%turn_id, "failed to backfill turn items for turn completion: {err}"),
    }
}

#[cfg(test)]
#[path = "history_backfill_tests.rs"]
mod tests;
