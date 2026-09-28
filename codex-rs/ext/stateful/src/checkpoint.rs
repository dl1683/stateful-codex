use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use codex_extension_api::ToolCallOutcome;

/// Completed direct tool calls after which the run World State raises one checkpoint nudge.
pub(crate) const CHECKPOINT_TOOL_CALLS: u64 = 8;

/// Per-thread run activity shared by the tool lifecycle hook, the run World State,
/// and the run tool, keyed by thread ID because tools cannot read thread stores.
#[derive(Clone, Default)]
pub(crate) struct RunActivityRegistry {
    threads: Arc<Mutex<HashMap<String, Arc<RunActivity>>>>,
}

impl RunActivityRegistry {
    pub(crate) fn for_thread(&self, thread_id: &str) -> Arc<RunActivity> {
        self.threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(thread_id.to_string())
            .or_default()
            .clone()
    }
}

/// Successful direct model tool calls since the active run's last obligation write.
/// Volatile by design: it only paces checkpoint nudges and resets on restart.
#[derive(Default)]
pub(crate) struct RunActivity {
    run_id: Mutex<Option<String>>,
    calls: AtomicU64,
}

impl RunActivity {
    /// Starts counting afresh whenever a different run becomes active on the thread.
    pub(crate) fn observe_run(&self, run_id: &str) {
        let mut current = self.run_id.lock().unwrap_or_else(PoisonError::into_inner);
        if current.as_deref() != Some(run_id) {
            *current = Some(run_id.to_string());
            self.calls.store(0, Ordering::Relaxed);
        }
    }

    /// Records one finished tool call. Only successful direct model calls count;
    /// nested code-mode calls are part of their outer call.
    pub(crate) fn record(
        &self,
        direct: bool,
        stateful_tool: Option<&str>,
        outcome: ToolCallOutcome,
    ) {
        if !direct || !matches!(outcome, ToolCallOutcome::Completed { success: true }) {
            return;
        }
        if stateful_tool == Some("obligation_update") {
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
