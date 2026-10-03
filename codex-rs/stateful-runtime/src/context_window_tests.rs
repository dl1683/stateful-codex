use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::ContextWindowDecision;
use crate::ContextWindowMode;
use crate::ContextWindowReason;
use crate::StatefulRunStore;

#[tokio::test]
async fn the_first_decision_for_a_window_is_final() {
    let home = TempDir::new().expect("state home");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store");
    let continuation = ContextWindowDecision {
        thread_id: "thread-1".to_string(),
        window_id: "window-2".to_string(),
        window_number: 1,
        mode: ContextWindowMode::Continuation,
        reason: ContextWindowReason::Compaction,
    };
    assert_eq!(
        store
            .context_window("thread-1", "window-2")
            .await
            .expect("read"),
        None
    );
    assert_eq!(
        store
            .decide_context_window(&continuation)
            .await
            .expect("decide"),
        continuation
    );
    let later = ContextWindowDecision {
        mode: ContextWindowMode::Full,
        reason: ContextWindowReason::Unknown,
        ..continuation.clone()
    };
    assert_eq!(
        store
            .decide_context_window(&later)
            .await
            .expect("decide again"),
        continuation
    );
    // A fork copies window identities; its own decisions are separate.
    let fork = ContextWindowDecision {
        thread_id: "thread-fork".to_string(),
        ..later
    };
    assert_eq!(
        store.decide_context_window(&fork).await.expect("fork"),
        fork
    );
}
