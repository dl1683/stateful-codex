//! Per-turn bound on rejected Stateful completion attempts.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

/// Rejected `stateful_run_update` completion attempts one turn may make before Stateful
/// stops offering completion. Every rejection names the wrong field and shows valid
/// calls filled with the run's current revisions, so a converging model succeeds within
/// one or two corrections. Three keeps that headroom while capping a model that is not
/// converging (for example one whose provider bridge reshapes its calls) at a few cheap
/// calls instead of the rest of its time budget. Once reached, `stateful_run_update` is
/// withheld from the thread's tools until its next turn starts.
pub(crate) const MAX_REJECTED_COMPLETIONS_PER_TURN: u32 = 3;
/// Threads tracked at once. Past this the counters are forgotten, which can only give a
/// turn a fresh allowance, never refuse a valid completion.
const MAX_TRACKED_THREADS: usize = 256;

/// Process-wide counts of rejected completion attempts, one current turn per thread.
#[derive(Clone, Default)]
pub(crate) struct CompletionAttempts {
    state: Arc<Mutex<HashMap<String, TurnAttempts>>>,
}

struct TurnAttempts {
    turn_id: String,
    rejected: u32,
}

impl CompletionAttempts {
    /// Rejected attempts the thread has made in `turn_id`.
    pub(crate) fn rejected(&self, thread_id: &str, turn_id: &str) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(thread_id)
            .filter(|attempts| attempts.turn_id == turn_id)
            .map_or(0, |attempts| attempts.rejected)
    }

    /// Whether the thread's current turn has used up its completion attempts. Cleared
    /// when the thread's next turn starts.
    pub(crate) fn exhausted(&self, thread_id: &str) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(thread_id)
            .is_some_and(|attempts| attempts.rejected >= MAX_REJECTED_COMPLETIONS_PER_TURN)
    }

    /// Counts one more rejected attempt in `turn_id` and returns the turn's total.
    pub(crate) fn reject(&self, thread_id: &str, turn_id: &str) -> u32 {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.len() >= MAX_TRACKED_THREADS && !state.contains_key(thread_id) {
            state.clear();
        }
        let attempts = state
            .entry(thread_id.to_string())
            .or_insert_with(|| TurnAttempts {
                turn_id: turn_id.to_string(),
                rejected: 0,
            });
        if attempts.turn_id != turn_id {
            *attempts = TurnAttempts {
                turn_id: turn_id.to_string(),
                rejected: 0,
            };
        }
        attempts.rejected = attempts.rejected.saturating_add(1);
        attempts.rejected
    }

    /// Forgets the thread's count once a completion is accepted or a new turn starts.
    pub(crate) fn clear(&self, thread_id: &str) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(thread_id);
    }
}
