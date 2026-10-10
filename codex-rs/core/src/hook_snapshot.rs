//! The hook set a turn dispatches is fixed when its task starts.
//!
//! Hook changes published while a turn runs (a config reload, a trust change) apply from the
//! next turn: every in-turn dispatch (session start, prompt submit, tool, permission,
//! compaction, stop, interrupt and after-agent hooks) uses the session hooks captured when the
//! turn's task started, after its plugin selection refreshed them. A host-ended answer
//! reservation relies on this: a turn that started with an empty set can run no hook.
//!
//! Executor-plugin hooks come from a step's capability-root discovery. A turn runs the ones its
//! first step discovered (including none), so a capability-root selection that changes
//! mid-turn applies from the next turn as well.

use std::sync::Arc;
use std::sync::OnceLock;

use codex_exec_server::ExecutorCapabilityDiscoverySnapshot;
use codex_hooks::Hooks;

use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn_context::TurnContext;

/// The hooks captured for one turn.
struct TurnHooks {
    session: Arc<Hooks>,
    /// The capability-root discovery of the turn's first step.
    executor: OnceLock<Option<Arc<ExecutorCapabilityDiscoverySnapshot>>>,
}

/// Fixes `turn`'s hook set to the session's current hooks.
pub(crate) fn pin_turn_hooks(sess: &Session, turn: &TurnContext) {
    turn.extension_data.insert(TurnHooks {
        session: sess.hooks(),
        executor: OnceLock::new(),
    });
}

/// Fixes `turn`'s executor-plugin hook sources to the first step's `discovery`; later steps
/// leave them unchanged.
pub(crate) fn pin_turn_executor_discovery(
    turn: &TurnContext,
    discovery: Option<&Arc<ExecutorCapabilityDiscoverySnapshot>>,
) {
    if let Some(pinned) = turn.extension_data.get::<TurnHooks>() {
        // Only the first step's discovery is kept.
        let _ = pinned.executor.set(discovery.cloned());
    }
}

/// The hooks to dispatch in `turn`: its pinned set, or the session's current hooks for a
/// context no task started.
pub(crate) fn turn_hooks(sess: &Session, turn: &TurnContext) -> Arc<Hooks> {
    turn.extension_data
        .get::<TurnHooks>()
        .map_or_else(|| sess.hooks(), |pinned| Arc::clone(&pinned.session))
}

/// The capability-root discovery whose executor-plugin hooks `step`'s turn runs: the turn's
/// first step's, or the step's own for a context no task started.
pub(crate) fn turn_executor_discovery(
    step: &StepContext,
) -> Option<Arc<ExecutorCapabilityDiscoverySnapshot>> {
    match step
        .turn
        .extension_data
        .get::<TurnHooks>()
        .and_then(|pinned| pinned.executor.get().cloned())
    {
        Some(discovery) => discovery,
        None => step.executor_capability_discovery.clone(),
    }
}

#[cfg(test)]
#[path = "hook_snapshot_tests.rs"]
mod tests;
