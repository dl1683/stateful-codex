use std::sync::Arc;
use std::sync::Mutex;

use super::*;
use crate::event_processor::EventProcessor;
use crate::event_processor_with_human_output::EventProcessorWithHumanOutput;
use crate::tests::TestLogWriter;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItemEntry;
use codex_app_server_protocol::TurnCompletedNotification;
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
        limit: Some(50),
        sort_direction: Some(SortDirection::Desc),
    }
}

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
async fn backfill_stops_at_exhaustion_and_repeated_cursor() {
    // A cursor that never advances is still bounded by the page budget.
    let mut requests = Vec::new();
    let items = newest_turn_items("thread-1", "turn-1", |params| {
        let index = requests.len();
        requests.push(params);
        std::future::ready(Ok(page(vec![plan(index)], Some("same"))))
    })
    .await;
    assert_eq!(items, Ok(vec![plan(/*index*/ 1), plan(/*index*/ 0)]));
    assert_eq!(
        requests,
        vec![items_params(/*cursor*/ None), items_params(Some("same"))]
    );
}

async fn complete_with_backfill(
    delivered: &str,
    backfill: Result<Vec<ThreadItem>, String>,
) -> (String, Vec<ThreadItem>, String) {
    let home = tempfile::tempdir().expect("codex home");
    let config = codex_core::config::ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .expect("config");
    let last_message_path = home.path().join("last-message.txt");
    let mut processor = EventProcessorWithHumanOutput::create_with_ansi(
        /*with_ansi*/ false,
        &config,
        Some(last_message_path.clone()),
    );
    processor.process_server_notification(ServerNotification::ItemCompleted(
        ItemCompletedNotification {
            item: agent_message(delivered.to_string()),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 0,
        },
    ));
    let mut turn = summary_turn("turn-1", TurnStatus::Completed);
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = TestLogWriter {
        buffer: Arc::clone(&buffer),
    };
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(writer)
        .finish();
    tracing::subscriber::with_default(subscriber, || apply_turn_backfill(&mut turn, backfill));
    let items = turn.items.clone();
    processor.process_server_notification(ServerNotification::TurnCompleted(
        TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn,
        },
    ));
    processor.print_final_output();
    let output = std::fs::read_to_string(last_message_path).expect("last message");
    let logs = String::from_utf8(buffer.lock().expect("log buffer").clone()).expect("utf8");
    (output, items, logs)
}

/// Also `backfill_follows_page_two`: the newest 100 items span two pages.
#[tokio::test]
async fn backfill_final_message_outside_suffix_is_not_recovered_and_warns() {
    let mut turn = (0..150).map(plan).collect::<Vec<_>>();
    turn[10] = agent_message("final answer".to_string());

    let (items, requests) = backfill_from(&turn).await;

    // Only the newest 100 items come back, in turn order, and none of them is the final message.
    assert_eq!(items, Ok(turn[50..].to_vec()));
    assert_eq!(
        requests,
        vec![items_params(/*cursor*/ None), items_params(Some("50"))]
    );
    // That incomplete suffix neither displaces the delivered answer nor lets a plan stand in.
    let (output, items, logs) = complete_with_backfill("final answer", items).await;
    assert_eq!((output.as_str(), items), ("final answer", Vec::new()));
    assert!(
        logs.contains("turn_id=turn-1")
            && logs.contains("no final agent message among the newest backfilled turn items"),
        "{logs}"
    );
}

#[tokio::test]
async fn backfill_unavailable_history_warns() {
    let error = Err("thread/items/list: unsupported".to_string());
    let (output, items, logs) = complete_with_backfill("delivered answer", error).await;
    assert_eq!((output.as_str(), items), ("delivered answer", Vec::new()));
    assert!(
        logs.contains("turn_id=turn-1")
            && logs.contains("failed to backfill turn items for turn completion"),
        "{logs}"
    );
}

#[tokio::test]
async fn backfill_large_single_item_is_passed_through() {
    // Pages and items are bounded, bytes are not (L3-3): the 1 MiB final message comes back whole.
    let final_message = "x".repeat(1024 * 1024);
    let turn = vec![plan(/*index*/ 0), agent_message(final_message.clone())];

    let (items, requests) = backfill_from(&turn).await;
    assert_eq!(requests, vec![items_params(/*cursor*/ None)]);
    let (output, items, logs) = complete_with_backfill("progress note", items).await;

    assert_eq!((output, items, logs), (final_message, turn, String::new()));
}

fn summary_turn(id: &str, status: TurnStatus) -> Turn {
    Turn {
        id: id.to_string(),
        items_view: TurnItemsView::Summary,
        items: Vec::new(),
        status,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    }
}

async fn reconcile_with(
    turns: Result<Vec<Turn>, String>,
    run: Result<Option<StatefulRun>, String>,
) -> (
    Result<AutonomousWaitState, String>,
    Vec<ThreadTurnsListParams>,
    bool,
) {
    let mut requests = Vec::new();
    let mut run_read = false;
    let outcome = reconcile_autonomous_wait(
        "thread-1",
        "turn-current",
        |params| {
            requests.push(params);
            std::future::ready(turns.map(|data| ThreadTurnsListResponse {
                data,
                next_cursor: Some("older".to_string()),
                backwards_cursor: None,
            }))
        },
        || {
            run_read = true;
            std::future::ready(run)
        },
    )
    .await;
    (outcome, requests, run_read)
}

#[tokio::test]
async fn reconcile_uses_summary_turns_only() {
    let eligible = summary_turn("turn-next", TurnStatus::InProgress);
    let newest = vec![
        summary_turn("turn-done", TurnStatus::Completed),
        summary_turn("turn-current", TurnStatus::InProgress),
        eligible.clone(),
    ];

    let (outcome, requests, run_read) = reconcile_with(Ok(newest), Ok(None)).await;

    assert_eq!(
        outcome,
        Ok(AutonomousWaitState::TurnStarted(Box::new(
            TurnStartedNotification {
                thread_id: "thread-1".to_string(),
                turn: eligible,
            }
        )))
    );
    let summary_page = ThreadTurnsListParams {
        thread_id: "thread-1".to_string(),
        cursor: None,
        limit: Some(4),
        sort_direction: Some(SortDirection::Desc),
        items_view: Some(TurnItemsView::Summary),
        persist_before_list: true,
    };
    assert_eq!((requests, run_read), (vec![summary_page], false));
}

#[tokio::test]
async fn reconcile_without_an_eligible_turn_reports_the_run_state() {
    let running = crate::tests::sample_stateful_run();
    let completed = StatefulRun {
        status: StatefulRunStatus::Completed,
        ..running.clone()
    };
    let collaborative = StatefulRun {
        mode: StatefulWorkflowMode::Collaborative,
        ..running.clone()
    };
    let no_eligible = || {
        Ok(vec![
            summary_turn("turn-current", TurnStatus::InProgress),
            summary_turn("turn-done", TurnStatus::Completed),
        ])
    };

    let mut outcomes = Vec::new();
    for run in [
        Ok(Some(running)),
        Ok(Some(completed)),
        Ok(Some(collaborative)),
        Ok(None),
        Err("statefulRun/read: unavailable".to_string()),
    ] {
        let (outcome, requests, run_read) = reconcile_with(no_eligible(), run).await;
        assert_eq!((requests.len(), run_read), (1, true));
        outcomes.push(outcome);
    }
    assert_eq!(
        outcomes,
        vec![
            Ok(AutonomousWaitState::Running),
            Ok(AutonomousWaitState::Terminal(StatefulRunStatus::Completed)),
            Ok(AutonomousWaitState::Terminal(StatefulRunStatus::Running)),
            Ok(AutonomousWaitState::Missing),
            Err("statefulRun/read: unavailable".to_string()),
        ]
    );
}
