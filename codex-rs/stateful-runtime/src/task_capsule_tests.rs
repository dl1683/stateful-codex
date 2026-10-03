use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use crate::NewWindowEvent;
use crate::StatefulRunStore;
use crate::TaskCapsule;
use crate::WindowEventKind;

#[tokio::test]
async fn a_window_keeps_its_first_capsule_and_later_edits_are_detected() {
    let home = TempDir::new().expect("home");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store");
    let first = TaskCapsule {
        through_seq: 0,
        body: "Next step: unknown.".to_string(),
    };
    assert_eq!(
        store
            .capture_task_capsule("thread-1", "window-1", &first)
            .await
            .expect("capture"),
        first
    );
    let later = TaskCapsule {
        through_seq: 4,
        body: "rebuilt".to_string(),
    };
    assert_eq!(
        store
            .capture_task_capsule("thread-1", "window-1", &later)
            .await
            .expect("capture again"),
        first
    );
    assert!(
        !store
            .edited_after("thread-1", "project-1", 0)
            .await
            .expect("edited")
    );
    store
        .append_window_event(&NewWindowEvent {
            thread_id: "thread-1".to_string(),
            event_key: "patch-1:edit".to_string(),
            project_id: "project-1".to_string(),
            turn_id: "turn-1".to_string(),
            kind: WindowEventKind::Edit,
            payload: json!({"status": "applied"}),
        })
        .await
        .expect("append");
    assert!(
        store
            .edited_after("thread-1", "project-1", 0)
            .await
            .expect("edited")
    );
    assert!(
        !store
            .edited_after("thread-1", "project-1", 1)
            .await
            .expect("edited")
    );
}
