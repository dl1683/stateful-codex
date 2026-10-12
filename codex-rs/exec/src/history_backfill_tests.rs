use super::*;
use codex_app_server_protocol::ThreadItemEntry;
use pretty_assertions::assert_eq;

fn plan(index: usize) -> ThreadItem {
    ThreadItem::Plan {
        id: format!("plan-{index}"),
        text: format!("step {index}"),
    }
}

fn agent_message(text: String) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: "final".to_string(),
        text,
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    }
}

fn page(items: Vec<ThreadItem>, next_cursor: Option<&str>) -> ThreadItemsListResponse {
    ThreadItemsListResponse {
        data: items
            .into_iter()
            .map(|item| ThreadItemEntry {
                turn_id: "turn-1".to_string(),
                item,
                started_at_ms: None,
                completed_at_ms: None,
            })
            .collect(),
        next_cursor: next_cursor.map(str::to_string),
        backwards_cursor: None,
    }
}

fn items_params(cursor: Option<&str>) -> ThreadItemsListParams {
    ThreadItemsListParams {
        thread_id: "thread-1".to_string(),
        turn_id: Some("turn-1".to_string()),
        cursor: cursor.map(|cursor| ThreadItemsListCursor::Opaque(cursor.to_string())),
        limit: Some(TURN_ITEMS_PAGE_LIMIT),
        sort_direction: Some(SortDirection::Desc),
    }
}

/// Serves `turn` (in turn order) newest first, using the item offset as the cursor.
fn serve_desc(turn: &[ThreadItem], params: &ThreadItemsListParams) -> ThreadItemsListResponse {
    let offset = match &params.cursor {
        None => 0,
        Some(ThreadItemsListCursor::Opaque(cursor)) => cursor.parse().expect("offset cursor"),
        Some(ThreadItemsListCursor::Anchor(anchor)) => panic!("unexpected anchor {anchor:?}"),
    };
    let limit = params.limit.expect("page limit") as usize;
    let end = (offset + limit).min(turn.len());
    let items = turn
        .iter()
        .rev()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();
    page(
        items,
        (end < turn.len()).then_some(end.to_string().as_str()),
    )
}

async fn backfill_from(
    turn: &[ThreadItem],
) -> (Result<Vec<ThreadItem>, String>, Vec<ThreadItemsListParams>) {
    let mut requests = Vec::new();
    let items = newest_turn_items("thread-1", "turn-1", |params| {
        let response = serve_desc(turn, &params);
        requests.push(params);
        std::future::ready(Ok(response))
    })
    .await;
    (items, requests)
}

#[tokio::test]
async fn backfill_follows_page_two() {
    let mut turn = (0..69).map(plan).collect::<Vec<_>>();
    turn.push(agent_message("done".to_string()));

    let (items, requests) = backfill_from(&turn).await;

    assert_eq!(items, Ok(turn));
    assert_eq!(requests, vec![items_params(None), items_params(Some("50"))]);
}

#[tokio::test]
async fn backfill_stops_at_exhaustion_and_repeated_cursor() {
    let turn = (0..10).map(plan).collect::<Vec<_>>();
    let (items, requests) = backfill_from(&turn).await;
    assert_eq!(items, Ok(turn));
    assert_eq!(requests, vec![items_params(None)]);

    // A cursor that never advances is still bounded by the page budget.
    let mut requests = Vec::new();
    let items = newest_turn_items("thread-1", "turn-1", |params| {
        let index = requests.len();
        requests.push(params);
        std::future::ready(Ok(page(vec![plan(index)], Some("same"))))
    })
    .await;
    assert_eq!(items, Ok(vec![plan(1), plan(0)]));
    assert_eq!(
        requests,
        vec![items_params(None), items_params(Some("same"))]
    );
}

#[tokio::test]
async fn backfill_final_message_outside_suffix_is_not_recovered() {
    let mut turn = (0..150).map(plan).collect::<Vec<_>>();
    turn[10] = agent_message("final answer".to_string());

    let (items, requests) = backfill_from(&turn).await;

    // Only the newest 100 items come back, and none of them is the final message, so exec warns.
    assert_eq!(items, Ok(turn[50..].to_vec()));
    assert_eq!(requests, vec![items_params(None), items_params(Some("50"))]);
}

#[tokio::test]
async fn backfill_unavailable_history_is_reported() {
    let mut calls = 0;
    let items = newest_turn_items("thread-1", "turn-1", |_| {
        calls += 1;
        std::future::ready(Err("thread/items/list: unsupported".to_string()))
    })
    .await;

    assert_eq!(items, Err("thread/items/list: unsupported".to_string()));
    assert_eq!(calls, 1);
}

#[tokio::test]
async fn backfill_large_single_item_is_passed_through() {
    // The bound is on pages and items, not bytes (limitation L3-3): a 1 MiB item comes back whole.
    let turn = vec![agent_message("x".repeat(1024 * 1024))];

    let (items, _) = backfill_from(&turn).await;

    assert_eq!(items, Ok(turn));
}

#[tokio::test]
async fn reconcile_uses_summary_turns_only() {
    let turn = |id: &str, status| Turn {
        id: id.to_string(),
        items_view: TurnItemsView::Summary,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    };
    let newest = vec![
        turn("turn-3", TurnStatus::InProgress),
        turn("turn-2", TurnStatus::InProgress),
        turn("turn-1", TurnStatus::Completed),
    ];
    let mut requests = Vec::new();

    let found = newest_other_in_progress_turn("thread-1", "turn-2", |params| {
        requests.push(params);
        std::future::ready(Ok(ThreadTurnsListResponse {
            data: newest.clone(),
            next_cursor: Some("older".to_string()),
            backwards_cursor: None,
        }))
    })
    .await;

    assert_eq!(found, Ok(Some(newest[0].clone())));
    assert_eq!(
        requests,
        vec![ThreadTurnsListParams {
            thread_id: "thread-1".to_string(),
            cursor: None,
            limit: Some(RECONCILE_TURNS_LIMIT),
            sort_direction: Some(SortDirection::Desc),
            items_view: Some(TurnItemsView::Summary),
        }]
    );
}
