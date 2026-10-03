use codex_extension_api::ContextWindowView;
use codex_extension_api::WindowBuild;
use codex_stateful_runtime::ContextWindowMode;
use codex_stateful_runtime::ContextWindowReason;
use pretty_assertions::assert_eq;
use serde_json::Map;
use serde_json::Value;

use super::proposed_window;
use super::window_section;

fn view(number: u64, build: WindowBuild) -> ContextWindowView {
    ContextWindowView {
        number,
        id: format!("window-{number}"),
        build,
    }
}

fn proposal(
    view: &ContextWindowView,
    previous: Option<&Map<String, Value>>,
) -> (ContextWindowMode, ContextWindowReason) {
    let decision = proposed_window("thread-1", view, previous);
    (decision.mode, decision.reason)
}

#[test]
fn boundaries_decide_by_how_the_window_opened() {
    let baseline = Map::new();
    assert_eq!(
        proposal(&view(3, WindowBuild::CompactionBoundary), Some(&baseline)),
        (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction
        )
    );
    // History was dropped without a summary: never a continuation.
    assert_eq!(
        proposal(&view(3, WindowBuild::ContextReset), Some(&baseline)),
        (ContextWindowMode::Full, ContextWindowReason::Reset)
    );
}

#[test]
fn ordinary_steps_follow_the_recorded_window_and_default_conservatively() {
    assert_eq!(
        proposal(&view(0, WindowBuild::OrdinaryStep), None),
        (ContextWindowMode::Full, ContextWindowReason::ThreadStart)
    );
    // Pre-turn or manual compaction installs no initial context and no baseline.
    assert_eq!(
        proposal(&view(2, WindowBuild::OrdinaryStep), None),
        (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction
        )
    );
    // A baseline without this thread's record for this window is unexplained.
    assert_eq!(
        proposal(&view(2, WindowBuild::OrdinaryStep), Some(&Map::new())),
        (ContextWindowMode::Full, ContextWindowReason::Unknown)
    );

    let continuation = proposed_window("thread-1", &view(2, WindowBuild::CompactionBoundary), None);
    let mut recorded = Map::new();
    recorded.insert(
        super::WORLD_STATE_ID.to_string(),
        window_section(&continuation).snapshot().clone(),
    );
    assert_eq!(
        proposal(&view(2, WindowBuild::OrdinaryStep), Some(&recorded)).0,
        ContextWindowMode::Continuation
    );
    // A fork copies the record but is another thread.
    assert_eq!(
        proposed_window(
            "thread-fork",
            &view(2, WindowBuild::OrdinaryStep),
            Some(&recorded)
        )
        .mode,
        ContextWindowMode::Full
    );
}
