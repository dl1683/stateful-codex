//! Only successful model obligation writes with submitted `next` guard this turn's final.

use codex_stateful_runtime::StatefulRunId;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

#[derive(Clone, Default)]
pub(crate) struct TurnProgress(Arc<Mutex<VecDeque<(String, StatefulRunId)>>>);

impl TurnProgress {
    pub(crate) fn record(&self, turn_id: &str, run_id: &StatefulRunId) {
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|(turn, _)| turn != turn_id);
        entries.push_back((turn_id.to_string(), run_id.clone()));
        while entries.len() > 64 {
            entries.pop_front();
        }
    }

    pub(crate) fn take(&self, turn_id: &str) -> Option<StatefulRunId> {
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let index = entries.iter().position(|(turn, _)| turn == turn_id)?;
        entries.remove(index).map(|(_, run)| run)
    }
}

#[cfg(test)]
#[path = "turn_progress_tests.rs"]
mod tests;
