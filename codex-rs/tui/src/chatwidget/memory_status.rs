//! The chat widget's side of the passive memory status: it keeps the last summary read for
//! its thread so the status line and `/status` can show it, and it asks the app to read the
//! summary again when a save, correction or commit observation is announced.

use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryCounts;
use codex_protocol::ThreadId;

use super::ChatWidget;
use crate::app_event::AppEvent;

impl ChatWidget {
    /// Stores the newest memory summary for `thread_id`; one for another thread is ignored.
    pub(crate) fn set_stateful_memory(
        &mut self,
        thread_id: ThreadId,
        counts: StatefulMemoryCounts,
        session: Option<StatefulMemoryChangeTotals>,
    ) {
        if self.thread_id != Some(thread_id) {
            return;
        }
        self.stateful_memory = Some((counts, session));
        self.refresh_status_line();
    }

    /// Asks the app to read the memory summary again after a change was announced.
    pub(super) fn request_memory_status_refresh(&self) {
        if let Some(thread_id) = self.thread_id {
            self.app_event_tx
                .send(AppEvent::StatefulMemoryRefresh { thread_id });
        }
    }

    /// The `/status` memory lines, when this is a Stateful session.
    pub(super) fn add_memory_status_output(&mut self) {
        if let Some((counts, session)) = self.stateful_memory {
            self.add_to_history(crate::memory_receipts::status_cell(
                &counts,
                session.as_ref(),
            ));
        }
    }
}
