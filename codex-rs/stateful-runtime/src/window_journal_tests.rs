use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::NewWindowEvent;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::WindowEventKind;
use crate::WindowPublication;
use crate::WindowPublicationState;

async fn store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store")
}

fn command(thread_id: &str, key: &str, exit_code: i64) -> NewWindowEvent {
    NewWindowEvent {
        thread_id: thread_id.to_string(),
        event_key: key.to_string(),
        project_id: "project-1".to_string(),
        turn_id: "turn-1".to_string(),
        kind: WindowEventKind::Command,
        payload: json!({"command": "cargo test", "exitCode": exit_code}),
    }
}

#[tokio::test]
async fn observations_survive_reopening_and_replays_are_idempotent() {
    let home = TempDir::new().expect("home");
    let store_a = store(&home).await;
    assert_eq!(
        store_a
            .append_window_event(&command("thread-1", "call-1:command", 1))
            .await
            .expect("append"),
        1
    );
    assert_eq!(
        store_a
            .append_window_event(&command("thread-1", "call-2:command", 0))
            .await
            .expect("append"),
        2
    );
    // A replay of the same observation is the same event.
    assert_eq!(
        store_a
            .append_window_event(&command("thread-1", "call-1:command", 1))
            .await
            .expect("replay"),
        1
    );
    // The same key cannot silently carry another observation.
    assert!(matches!(
        store_a
            .append_window_event(&command("thread-1", "call-1:command", 0))
            .await,
        Err(StatefulRunStoreError::WindowEventConflict(_))
    ));
    // Sequences are per thread.
    assert_eq!(
        store_a
            .append_window_event(&command("thread-2", "call-1:command", 0))
            .await
            .expect("other thread"),
        1
    );
    drop(store_a);

    let reopened = store(&home).await;
    assert_eq!(
        reopened
            .window_event_watermark("thread-1", "project-1")
            .await
            .expect("watermark"),
        2
    );
    let events = reopened
        .window_events_newest_first("thread-1", "project-1", 0, 2, 10)
        .await
        .expect("events");
    assert_eq!(
        events
            .iter()
            .map(|event| (event.seq, event.event.clone()))
            .collect::<Vec<_>>(),
        vec![
            (2, command("thread-1", "call-2:command", 0)),
            (1, command("thread-1", "call-1:command", 1)),
        ]
    );
}

#[tokio::test]
async fn publications_cover_disjoint_suffixes_and_retry_the_frozen_content() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    for (key, exit_code) in [("a", 0), ("b", 1)] {
        store
            .append_window_event(&command("thread-1", key, exit_code))
            .await
            .expect("append");
    }
    assert_eq!(
        store
            .threads_with_unpublished_events("project-1", i64::MAX, 10)
            .await
            .expect("threads"),
        vec!["thread-1".to_string()]
    );
    let first = WindowPublication {
        thread_id: "thread-1".to_string(),
        from_seq: 0,
        through_seq: 2,
        project_id: "project-1".to_string(),
        entry_id: "entry-1".to_string(),
        content: "receipts 1-2".to_string(),
        state: WindowPublicationState::Pending,
    };
    assert_eq!(
        store.stage_window_publication(&first).await.expect("stage"),
        first
    );
    // A retry after a crash returns the frozen publication, even with other content.
    let retried = WindowPublication {
        content: "rebuilt later".to_string(),
        ..first.clone()
    };
    assert_eq!(
        store
            .stage_window_publication(&retried)
            .await
            .expect("retry"),
        first
    );
    // A suffix that does not start where the last publication ended is refused.
    let overlapping = WindowPublication {
        from_seq: 1,
        through_seq: 3,
        ..first.clone()
    };
    assert!(matches!(
        store.stage_window_publication(&overlapping).await,
        Err(StatefulRunStoreError::ConcurrentMutation)
    ));
    assert_eq!(
        store
            .pending_window_publications("project-1", 10)
            .await
            .expect("pending"),
        vec![first.clone()]
    );
    store
        .mark_window_publication_published("thread-1", "project-1", 0)
        .await
        .expect("published");
    assert_eq!(
        store
            .pending_window_publications("project-1", 10)
            .await
            .expect("pending"),
        Vec::new()
    );
    assert_eq!(
        store
            .threads_with_unpublished_events("project-1", i64::MAX, 10)
            .await
            .expect("threads"),
        Vec::<String>::new()
    );
    assert_eq!(
        store
            .window_publication_watermark("thread-1", "project-1")
            .await
            .expect("watermark"),
        2
    );
}

#[tokio::test]
async fn a_thread_moved_between_projects_publishes_each_projects_work_to_it_alone() {
    let home = TempDir::new().expect("home");
    let store = store(&home).await;
    store
        .append_window_event(&command("thread-1", "a", 0))
        .await
        .expect("project 1");
    store
        .append_window_event(&NewWindowEvent {
            project_id: "project-2".to_string(),
            ..command("thread-1", "b", 1)
        })
        .await
        .expect("project 2");
    assert_eq!(
        store
            .window_event_watermark("thread-1", "project-2")
            .await
            .expect("watermark"),
        2
    );
    let second = store
        .window_events_newest_first("thread-1", "project-2", 0, 2, 10)
        .await
        .expect("events");
    assert_eq!(
        second.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![2]
    );
    let publication = WindowPublication {
        thread_id: "thread-1".to_string(),
        from_seq: 0,
        through_seq: 2,
        project_id: "project-2".to_string(),
        entry_id: "entry-2".to_string(),
        content: "project 2 receipts".to_string(),
        state: WindowPublicationState::Pending,
    };
    store
        .stage_window_publication(&publication)
        .await
        .expect("stage");
    // Project 1's observation is still unpublished, and recent activity counts as live.
    assert_eq!(
        store
            .threads_with_unpublished_events("project-1", i64::MAX, 10)
            .await
            .expect("threads"),
        vec!["thread-1".to_string()]
    );
    assert_eq!(
        store
            .threads_with_unpublished_events("project-1", 0, 10)
            .await
            .expect("threads"),
        Vec::<String>::new()
    );
    // A replay with the same key cannot move an observation to another project.
    assert!(matches!(
        store
            .append_window_event(&NewWindowEvent {
                project_id: "project-2".to_string(),
                ..command("thread-1", "a", 0)
            })
            .await,
        Err(StatefulRunStoreError::WindowEventConflict(_))
    ));
}
