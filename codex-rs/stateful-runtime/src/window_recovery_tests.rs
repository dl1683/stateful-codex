use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::IdleStaging;
use crate::NewWindowEvent;
use crate::StatefulRunStore;
use crate::WindowEventKind;
use crate::WindowPublication;
use crate::WindowPublicationState;

async fn store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store")
}

fn event(thread_id: &str, key: &str) -> NewWindowEvent {
    NewWindowEvent {
        thread_id: thread_id.to_string(),
        event_key: key.to_string(),
        project_id: "project-1".to_string(),
        turn_id: "turn-1".to_string(),
        kind: WindowEventKind::Command,
        payload: json!({"command": "cargo test", "exitCode": 0}),
    }
}

fn publication(thread_id: &str, from_seq: u64, through_seq: u64) -> WindowPublication {
    WindowPublication {
        thread_id: thread_id.to_string(),
        from_seq,
        through_seq,
        project_id: "project-1".to_string(),
        entry_id: format!("entry-{thread_id}-{from_seq}"),
        content: "receipts".to_string(),
        state: WindowPublicationState::Pending,
    }
}

#[tokio::test]
async fn closures_are_durable_and_monotonic() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    assert_eq!(
        store
            .latest_window_closure("thread-1", "project-1")
            .await
            .expect("none"),
        0
    );
    for through in [3, 7, 7] {
        store
            .record_window_closure("thread-1", "project-1", through)
            .await
            .expect("record");
    }
    assert_eq!(
        store
            .latest_window_closure("thread-1", "project-1")
            .await
            .expect("latest"),
        7
    );
}

#[tokio::test]
async fn a_thread_that_became_active_after_selection_is_not_staged() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    store
        .append_window_event(&event("thread-1", "a"))
        .await
        .expect("a");
    // Selected as idle through seq 1, then it records again before staging.
    store
        .append_window_event(&event("thread-1", "b"))
        .await
        .expect("b");
    assert_eq!(
        store
            .stage_idle_window_publication(&publication("thread-1", 0, 1), i64::MAX)
            .await
            .expect("checked"),
        IdleStaging::BecameActive
    );
    // Recent activity also counts, even at the selected sequence.
    assert_eq!(
        store
            .stage_idle_window_publication(&publication("thread-1", 0, 2), 0)
            .await
            .expect("checked"),
        IdleStaging::BecameActive
    );
    assert_eq!(
        store
            .stage_idle_window_publication(&publication("thread-1", 0, 2), i64::MAX)
            .await
            .expect("staged"),
        IdleStaging::Staged
    );
}

#[tokio::test]
async fn pending_pages_move_past_rows_that_keep_failing() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    for index in 0..3 {
        let thread = format!("thread-{index}");
        store
            .append_window_event(&event(&thread, "a"))
            .await
            .expect("append");
        store
            .stage_window_publication(&publication(&thread, 0, 1))
            .await
            .expect("stage");
    }
    let first = store
        .pending_window_publications_after("project-1", 0, 2)
        .await
        .expect("first page");
    assert_eq!(first.len(), 2);
    let after = first.last().expect("cursor").0;
    let second = store
        .pending_window_publications_after("project-1", after, 2)
        .await
        .expect("second page");
    assert_eq!(
        second
            .iter()
            .map(|(_, publication)| publication.thread_id.clone())
            .collect::<Vec<_>>(),
        vec!["thread-2".to_string()]
    );
}
