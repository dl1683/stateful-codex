//! Per-turn cost attribution recorded where the work happens and reported with the turn's
//! attribution summary.
//!
//! Tool counts alone do not say where an opening's premium goes. The packet records the
//! bytes of the project and continuity sections it renders (direct byte counts, not token
//! estimates); memory writes record records that were already present or refused; recipe
//! grounding records recipes tied to an observed command; explicit reads and on-demand
//! indexing record index operations, the ones still pending at their deadline, and the
//! time callers waited. Nothing here enters the model's context.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

/// Turns tracked at once; a turn whose summary never drains its counters cannot grow
/// the ledger without bound.
const MAX_TRACKED_TURNS: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CostCounters {
    /// Largest rendered project packet section this turn, in bytes.
    pub(crate) packet_project_bytes: u64,
    /// Largest rendered continuity record this turn, in bytes.
    pub(crate) packet_continuity_bytes: u64,
    pub(crate) memory_records_already_present: u64,
    pub(crate) memory_records_refused: u64,
    pub(crate) recipes_grounded: u64,
    pub(crate) index_operations: u64,
    pub(crate) index_operations_pending: u64,
    pub(crate) index_wait_ms: u64,
}

#[derive(Clone, Default)]
pub(crate) struct CostLedger {
    turns: Arc<Mutex<HashMap<String, CostCounters>>>,
}

impl CostLedger {
    pub(crate) fn record(&self, turn_id: &str, update: impl FnOnce(&mut CostCounters)) {
        let mut turns = self
            .turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !turns.contains_key(turn_id) && turns.len() >= MAX_TRACKED_TURNS {
            turns.clear();
        }
        update(turns.entry(turn_id.to_string()).or_default());
    }

    /// The turn's counters, removed from the ledger.
    pub(crate) fn take(&self, turn_id: &str) -> CostCounters {
        self.turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(turn_id)
            .unwrap_or_default()
    }
}

/// Records one index operation a caller waited `waited` for.
pub(crate) fn record_index_operation(
    ledger: &CostLedger,
    turn_id: &str,
    waited: std::time::Duration,
    pending: bool,
) {
    ledger.record(turn_id, |counters| {
        counters.index_operations += 1;
        counters.index_operations_pending += u64::from(pending);
        counters.index_wait_ms = counters
            .index_wait_ms
            .saturating_add(u64::try_from(waited.as_millis()).unwrap_or(u64::MAX));
    });
}
