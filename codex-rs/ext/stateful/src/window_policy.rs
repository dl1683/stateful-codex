//! Decides, once per context window and selected project, whether Stateful renders its full
//! packets or a continuation beside a native compaction summary.
//!
//! The decision is stored per (thread, project, window) when the window opens, so ordinary
//! steps, retries and same-thread resumes render the window the same way. An undecided window
//! is classified from the thread's own records, never from whether a baseline happens to exist:
//! a thread that already opened windows for this project and meets an undecided one was
//! compacted with its initial context deferred; a thread with no record is a new thread, a fork
//! or a newly selected project, and starts in full unless it inherited a carrier.

use codex_extension_api::ContextWindowView;
use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::WindowBuild;
use codex_extension_api::WorldStateSectionContribution;
use codex_stateful_runtime::ContextWindowDecision;
use codex_stateful_runtime::ContextWindowMode;
use codex_stateful_runtime::ContextWindowReason;
use codex_stateful_runtime::StatefulRunStore;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

/// An invisible section recording the window decision in the persisted World State: it lets a
/// fork keep the carrier it inherited, and it decides when the runtime store cannot be read.
pub(crate) const WORLD_STATE_ID: &str = "stateful_window";

/// Where a thread stands for the selected project, from its own durable records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ThreadRecord {
    /// This thread already opened a window for this project.
    Known,
    /// No window of this thread was recorded for this project, or the store is unreadable.
    Unrecorded,
}

/// The decision this step proposes when the window has none yet. `Continuation` is only a
/// proposal for a new window: the caller keeps the full packet when the continuation cannot
/// carry every rule whole.
pub(crate) fn proposed_window(
    thread_id: &str,
    project_id: &str,
    view: &ContextWindowView,
    previous_world_state: Option<&Map<String, Value>>,
    record: ThreadRecord,
) -> ContextWindowDecision {
    let (mode, reason) = match view.build {
        WindowBuild::CompactionBoundary => (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction,
        ),
        WindowBuild::ContextReset => (ContextWindowMode::Full, ContextWindowReason::Reset),
        WindowBuild::OrdinaryStep => {
            match (recorded(project_id, view, previous_world_state), record) {
                // This window's own record, or a carrier a fork inherited with its history: keep
                // rendering what the model already holds.
                (Some(mode), _) => (mode, ContextWindowReason::Unknown),
                (None, _) if view.number == 0 => {
                    (ContextWindowMode::Full, ContextWindowReason::ThreadStart)
                }
                // A compaction that deferred its initial context to this step.
                (None, ThreadRecord::Known) => (
                    ContextWindowMode::Continuation,
                    ContextWindowReason::Compaction,
                ),
                (None, ThreadRecord::Unrecorded) => {
                    (ContextWindowMode::Full, ContextWindowReason::Unknown)
                }
            }
        }
    };
    ContextWindowDecision {
        thread_id: thread_id.to_string(),
        project_id: project_id.to_string(),
        window_id: view.id.clone(),
        window_number: view.number,
        mode,
        reason,
    }
}

/// The stored decision for this window, if the runtime store has one.
pub(crate) async fn stored_window(
    store: Option<&StatefulRunStore>,
    thread_id: &str,
    project_id: &str,
    view: &ContextWindowView,
) -> (Option<ContextWindowDecision>, ThreadRecord) {
    let Some(store) = store.filter(|_| !view.id.is_empty()) else {
        return (None, ThreadRecord::Unrecorded);
    };
    let stored = match store.context_window(thread_id, project_id, &view.id).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::warn!(%thread_id, window_id = %view.id, %error, "failed to read a context window decision");
            return (None, ThreadRecord::Unrecorded);
        }
    };
    let record = match store
        .thread_has_context_windows(thread_id, project_id)
        .await
    {
        Ok(true) => ThreadRecord::Known,
        Ok(false) => ThreadRecord::Unrecorded,
        Err(error) => {
            tracing::warn!(%thread_id, %error, "failed to read a thread's window records");
            ThreadRecord::Unrecorded
        }
    };
    (stored, record)
}

/// Stores a new window's decision; the first stored decision wins and is returned.
pub(crate) async fn decide_window(
    store: Option<&StatefulRunStore>,
    decision: ContextWindowDecision,
) -> ContextWindowDecision {
    let Some(store) = store.filter(|_| !decision.window_id.is_empty()) else {
        return decision;
    };
    match store.decide_context_window(&decision).await {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(thread_id = %decision.thread_id, window_id = %decision.window_id, %error, "failed to store a context window decision");
            decision
        }
    }
}

/// The invisible section that carries the decision in the persisted World State.
pub(crate) fn window_section(decision: &ContextWindowDecision) -> WorldStateSectionContribution {
    let snapshot = json!({
        "threadId": decision.thread_id,
        "projectId": decision.project_id,
        "windowId": decision.window_id,
        "mode": mode_name(decision.mode),
    });
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot, |_: PreviousWorldStateSection| {
        None
    })
}

/// The mode recorded for this project and window in the persisted World State, by this
/// thread or (for a fork, which copies its parent's history and window identity) its parent.
fn recorded(
    project_id: &str,
    view: &ContextWindowView,
    previous_world_state: Option<&Map<String, Value>>,
) -> Option<ContextWindowMode> {
    let recorded = previous_world_state?.get(WORLD_STATE_ID)?;
    if recorded.get("projectId")?.as_str()? != project_id
        || recorded.get("windowId")?.as_str()? != view.id
    {
        return None;
    }
    match recorded.get("mode")?.as_str()? {
        "full" => Some(ContextWindowMode::Full),
        "continuation" => Some(ContextWindowMode::Continuation),
        _ => None,
    }
}

fn mode_name(mode: ContextWindowMode) -> &'static str {
    match mode {
        ContextWindowMode::Full => "full",
        ContextWindowMode::Continuation => "continuation",
    }
}

#[cfg(test)]
#[path = "window_policy_tests.rs"]
mod tests;
