use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_extension_api::ToolCallOutcome;

/// Completed tool calls after which the run World State raises one checkpoint nudge.
pub(crate) const CHECKPOINT_TOOL_CALLS: u64 = 8;

/// Counts completed tool calls since the thread's last successful semantic
/// obligation write, so the run World State can ask for one bounded checkpoint per
/// epoch instead of relying on the model to notice its plan went stale.
#[derive(Default)]
pub(crate) struct CheckpointCounter {
    calls: AtomicU64,
}

impl CheckpointCounter {
    /// Records one finished tool call; a successful obligation or run update resets.
    pub(crate) fn record(&self, stateful_tool: Option<&str>, outcome: ToolCallOutcome) {
        let wrote_obligation = matches!(
            stateful_tool,
            Some("obligation_update" | "stateful_run_update")
        ) && matches!(outcome, ToolCallOutcome::Completed { success: true });
        if wrote_obligation {
            self.calls.store(0, Ordering::Relaxed);
        } else {
            self.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The current checkpoint epoch once at least one threshold has been crossed.
    /// It changes only at threshold boundaries, keeping World State cache-stable.
    pub(crate) fn due_epoch(&self) -> Option<u64> {
        let calls = self.calls.load(Ordering::Relaxed);
        (calls >= CHECKPOINT_TOOL_CALLS).then_some(calls / CHECKPOINT_TOOL_CALLS)
    }
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
