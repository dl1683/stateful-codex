//! Decides, once per context window, whether Stateful renders its full packets or a
//! continuation beside a native compaction summary.
//!
//! The decision is stored per (thread, window) when the window opens, so ordinary steps,
//! retries and same-thread resumes all render the window the same way. A fork copies window
//! identities but not this thread-scoped record, so it starts with the full packets.

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

/// An invisible section recording the window decision in the persisted World State, used
/// when the runtime store cannot be read.
pub(crate) const WORLD_STATE_ID: &str = "stateful_window";

/// The decision this step proposes when the window has none yet. `Continuation` is only a
/// proposal: the caller keeps the full packet when the continuation cannot carry every rule.
pub(crate) fn proposed_window(
    thread_id: &str,
    view: &ContextWindowView,
    previous_world_state: Option<&Map<String, Value>>,
) -> ContextWindowDecision {
    let (mode, reason) = match view.build {
        WindowBuild::CompactionBoundary => (
            ContextWindowMode::Continuation,
            ContextWindowReason::Compaction,
        ),
        WindowBuild::ContextReset => (ContextWindowMode::Full, ContextWindowReason::Reset),
        WindowBuild::OrdinaryStep => match recorded_mode(thread_id, view, previous_world_state) {
            Some(mode) => (mode, ContextWindowReason::Unknown),
            None if view.number == 0 => (ContextWindowMode::Full, ContextWindowReason::ThreadStart),
            // Compaction that defers its initial context (pre-turn or manual) leaves no
            // baseline; the next ordinary step opens the window it installed.
            None if previous_world_state.is_none() => (
                ContextWindowMode::Continuation,
                ContextWindowReason::Compaction,
            ),
            // Anything else that is unexplained keeps the full packet.
            None => (ContextWindowMode::Full, ContextWindowReason::Unknown),
        },
    };
    ContextWindowDecision {
        thread_id: thread_id.to_string(),
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
    view: &ContextWindowView,
) -> Option<ContextWindowDecision> {
    let store = store?;
    if view.id.is_empty() {
        return None;
    }
    match store.context_window(thread_id, &view.id).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::warn!(%thread_id, window_id = %view.id, %error, "failed to read a context window decision");
            None
        }
    }
}

/// Stores a new window's decision; the first stored decision wins and is returned.
pub(crate) async fn decide_window(
    store: Option<&StatefulRunStore>,
    decision: ContextWindowDecision,
) -> ContextWindowDecision {
    let Some(store) = store else {
        return decision;
    };
    if decision.window_id.is_empty() {
        return decision;
    }
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
        "windowId": decision.window_id,
        "mode": mode_name(decision.mode),
    });
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot, |_: PreviousWorldStateSection| {
        None
    })
}

fn recorded_mode(
    thread_id: &str,
    view: &ContextWindowView,
    previous_world_state: Option<&Map<String, Value>>,
) -> Option<ContextWindowMode> {
    let recorded = previous_world_state?.get(WORLD_STATE_ID)?;
    if recorded.get("threadId")?.as_str()? != thread_id
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
