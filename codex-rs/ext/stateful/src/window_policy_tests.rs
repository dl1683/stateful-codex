use codex_extension_api::ContextWindowView;
use codex_extension_api::WindowBuild;
use codex_stateful_runtime::ContextWindowDecision;
use codex_stateful_runtime::ContextWindowMode;
use codex_stateful_runtime::ContextWindowReason;
use pretty_assertions::assert_eq;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use super::ThreadRecord;
use super::proposed_window;
use super::settled_window;
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
    record: ThreadRecord,
) -> (ContextWindowMode, ContextWindowReason) {
    let decision = proposed_window("thread-1", "project-1", view, record);
    (decision.mode, decision.reason)
}

fn installed(
    thread_id: &str,
    project_id: &str,
    number: u64,
    mode: ContextWindowMode,
) -> Map<String, Value> {
    let marker = ContextWindowDecision {
        thread_id: thread_id.to_string(),
        project_id: project_id.to_string(),
        window_id: format!("window-{number}"),
        window_number: number,
        mode,
        reason: ContextWindowReason::Compaction,
    };
    Map::from_iter([(
        super::WORLD_STATE_ID.to_string(),
        window_section(&marker).snapshot().clone(),
    )])
}

fn settled(
    project_id: &str,
    view: &ContextWindowView,
    previous: Option<&Map<String, Value>>,
    stored: Option<ContextWindowMode>,
) -> Option<ContextWindowMode> {
    let stored = stored.map(|mode| ContextWindowDecision {
        thread_id: "thread-1".to_string(),
        project_id: project_id.to_string(),
        window_id: view.id.clone(),
        window_number: view.number,
        mode,
        reason: ContextWindowReason::Compaction,
    });
    settled_window("thread-1", project_id, view, previous, stored).map(|decision| decision.mode)
}

#[test]
fn boundaries_decide_by_how_the_window_opened() {
    assert_eq!(
        proposal(
            &view(3, WindowBuild::CompactionBoundary),
            ThreadRecord::Known
        ),
        (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction
        )
    );
    // History was dropped without a summary: never a continuation.
    assert_eq!(
        proposal(&view(3, WindowBuild::ContextReset), ThreadRecord::Known),
        (ContextWindowMode::Full, ContextWindowReason::Reset)
    );
}

#[test]
fn undecided_windows_are_classified_from_the_threads_own_records() {
    assert_eq!(
        proposal(
            &view(0, WindowBuild::OrdinaryStep),
            ThreadRecord::Unrecorded
        ),
        (ContextWindowMode::Full, ContextWindowReason::ThreadStart)
    );
    // A compaction that deferred its initial context, whether or not another extension kept
    // metadata in the baseline: nothing of Stateful's is installed, so nothing is settled.
    let other_extension = Map::from_iter([("host_skills".to_string(), json!({"cloud": 1}))]);
    for previous in [None, Some(&other_extension)] {
        assert_eq!(
            settled(
                "project-1",
                &view(2, WindowBuild::OrdinaryStep),
                previous,
                None
            ),
            None
        );
    }
    assert_eq!(
        proposal(&view(2, WindowBuild::OrdinaryStep), ThreadRecord::Known),
        (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction
        )
    );
    // A fork before the parent's deferred injection, a newly selected project, or a thread
    // recorded before windows were: the full packet.
    assert_eq!(
        proposal(
            &view(2, WindowBuild::OrdinaryStep),
            ThreadRecord::Unrecorded
        ),
        (ContextWindowMode::Full, ContextWindowReason::Unknown)
    );
}

#[test]
fn what_is_installed_settles_ordinary_steps_before_any_stored_decision() {
    let ordinary = view(2, WindowBuild::OrdinaryStep);
    // A fork keeps the carrier it inherited with its parent's history.
    let parent = installed(
        "thread-parent",
        "project-1",
        2,
        ContextWindowMode::Continuation,
    );
    assert_eq!(
        settled("project-1", &ordinary, Some(&parent), None),
        Some(ContextWindowMode::Continuation)
    );
    // Returning to a project after another project's packet was installed starts in full,
    // even when this project's window decision was a continuation.
    let other = installed("thread-1", "project-2", 2, ContextWindowMode::Full);
    assert_eq!(
        settled(
            "project-1",
            &ordinary,
            Some(&other),
            Some(ContextWindowMode::Continuation)
        ),
        Some(ContextWindowMode::Full)
    );
    // Once that full packet is installed, the window keeps it.
    let returned = installed("thread-1", "project-1", 2, ContextWindowMode::Full);
    assert_eq!(
        settled(
            "project-1",
            &ordinary,
            Some(&returned),
            Some(ContextWindowMode::Continuation)
        ),
        Some(ContextWindowMode::Full)
    );
    // A boundary build never inherits: the stored decision (if any) or a proposal decides.
    assert_eq!(
        settled(
            "project-1",
            &view(2, WindowBuild::CompactionBoundary),
            Some(&returned),
            None
        ),
        None
    );
}
