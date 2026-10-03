use codex_extension_api::ContextWindowView;
use codex_extension_api::WindowBuild;
use codex_stateful_runtime::ContextWindowMode;
use codex_stateful_runtime::ContextWindowReason;
use pretty_assertions::assert_eq;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use super::ThreadRecord;
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
    record: ThreadRecord,
) -> (ContextWindowMode, ContextWindowReason) {
    let decision = proposed_window("thread-1", "project-1", view, previous, record);
    (decision.mode, decision.reason)
}

#[test]
fn boundaries_decide_by_how_the_window_opened() {
    assert_eq!(
        proposal(
            &view(3, WindowBuild::CompactionBoundary),
            Some(&Map::new()),
            ThreadRecord::Known
        ),
        (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction
        )
    );
    // History was dropped without a summary: never a continuation.
    assert_eq!(
        proposal(
            &view(3, WindowBuild::ContextReset),
            Some(&Map::new()),
            ThreadRecord::Known
        ),
        (ContextWindowMode::Full, ContextWindowReason::Reset)
    );
}

#[test]
fn undecided_windows_are_classified_from_the_threads_own_records() {
    assert_eq!(
        proposal(
            &view(0, WindowBuild::OrdinaryStep),
            None,
            ThreadRecord::Unrecorded
        ),
        (ContextWindowMode::Full, ContextWindowReason::ThreadStart)
    );
    // A compaction that deferred its initial context, whether or not another extension kept
    // metadata in the baseline.
    let other_extension = Map::from_iter([("host_skills".to_string(), json!({"cloud": 1}))]);
    for previous in [None, Some(&other_extension)] {
        assert_eq!(
            proposal(
                &view(2, WindowBuild::OrdinaryStep),
                previous,
                ThreadRecord::Known
            ),
            (
                ContextWindowMode::Continuation,
                ContextWindowReason::Compaction
            )
        );
    }
    // A fork before the parent's deferred injection, a newly selected project, or a thread
    // recorded before windows were: the full packet.
    for previous in [None, Some(&other_extension)] {
        assert_eq!(
            proposal(
                &view(2, WindowBuild::OrdinaryStep),
                previous,
                ThreadRecord::Unrecorded
            ),
            (ContextWindowMode::Full, ContextWindowReason::Unknown)
        );
    }
}

#[test]
fn a_fork_keeps_the_carrier_it_inherited_and_another_project_does_not() {
    let parent = proposed_window(
        "thread-parent",
        "project-1",
        &view(2, WindowBuild::CompactionBoundary),
        None,
        ThreadRecord::Known,
    );
    let inherited = Map::from_iter([(
        super::WORLD_STATE_ID.to_string(),
        window_section(&parent).snapshot().clone(),
    )]);
    assert_eq!(
        proposal(
            &view(2, WindowBuild::OrdinaryStep),
            Some(&inherited),
            ThreadRecord::Unrecorded
        ),
        (
            ContextWindowMode::Continuation,
            ContextWindowReason::Unknown
        )
    );
    assert_eq!(
        proposed_window(
            "thread-parent",
            "project-2",
            &view(2, WindowBuild::OrdinaryStep),
            Some(&inherited),
            ThreadRecord::Unrecorded,
        )
        .mode,
        ContextWindowMode::Full
    );
}
