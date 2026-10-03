//! The chat widget's side of the passive memory status: it keeps the last summary read for
//! its thread so the status line and `/status` can show it, and it asks the app to read the
//! summary again when a save, correction or commit observation is announced.

use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryCounts;
use codex_protocol::ThreadId;

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::memory_receipts::MemoryView;

impl ChatWidget {
    /// Stores the newest memory summary for `thread_id`; one for another thread is ignored.
    pub(crate) fn set_stateful_memory(
        &mut self,
        thread_id: ThreadId,
        counts: StatefulMemoryCounts,
        session: Option<StatefulMemoryChangeTotals>,
    ) {
        self.set_memory_view(thread_id, MemoryView::Known { counts, session });
    }

    /// Records that `thread_id`'s project memory could not be read.
    pub(crate) fn set_stateful_memory_unavailable(&mut self, thread_id: ThreadId) {
        self.set_memory_view(thread_id, MemoryView::Unavailable);
    }

    fn set_memory_view(&mut self, thread_id: ThreadId, view: MemoryView) {
        if self.thread_id != Some(thread_id) {
            return;
        }
        self.stateful_memory = Some(view);
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
        if let Some(view) = self.stateful_memory {
            self.add_to_history(view.status_cell());
        }
    }
}
