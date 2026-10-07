//! Live conversation packet: captured from a compaction's unmodified input, installed with its
//! checkpoint, kept by checkpoints that do not capture, and cleared by destructive replacement.
//! Snapshots share the immutable packet. It is host data and never enters model context here.

use std::sync::Arc;

use codex_history::ConversationPacket;
use codex_history::ConversationPacketSource;
use codex_history::HistoryContinuity;
use codex_history::assemble_conversation_packet;
use codex_protocol::ThreadId;
use tracing::warn;

use super::ContextManager;

/// What a new history checkpoint does with the live conversation packet.
pub(crate) enum ConversationPacketUpdate {
    /// Install the packet captured from this compaction's input history.
    Captured(ConversationPacketCapture),
    /// Keep the live packet, for checkpoints that capture nothing themselves.
    CarryForward,
    /// Drop the live packet with a destructive history replacement.
    Clear,
}

pub(crate) struct ConversationPacketCapture {
    packet: Option<Arc<ConversationPacket>>,
    /// A destructive reset after the capture makes it describe a discarded history.
    reset_version: u64,
}

impl ContextManager {
    pub(crate) fn conversation_packet(&self) -> Option<&Arc<ConversationPacket>> {
        self.conversation_packet.as_ref()
    }

    /// Captures from this snapshot, which must be the compaction's unmodified input. Callers
    /// clone history under the session lock and assemble outside it.
    pub(crate) fn capture_conversation_packet(
        &self,
        thread_id: ThreadId,
    ) -> ConversationPacketUpdate {
        let continuity = if self.history_version == 0 {
            HistoryContinuity::SinceThreadStart
        } else {
            HistoryContinuity::Rewritten
        };
        match assemble_conversation_packet(ConversationPacketSource {
            thread_id,
            history: &self.items,
            retained: &self.retained_context,
            previous: self.conversation_packet.as_deref(),
            continuity,
        }) {
            Ok(packet) => ConversationPacketUpdate::Captured(ConversationPacketCapture {
                packet: packet.map(Arc::new),
                reset_version: self.reset_version,
            }),
            Err(error) => {
                // Inconsistent host evidence must not erase the packet already installed.
                warn!(%error, "keeping the previous conversation packet");
                ConversationPacketUpdate::CarryForward
            }
        }
    }

    /// Applies a checkpoint's disposition before its history replacement, returning the exact
    /// packet that checkpoint persists.
    pub(crate) fn update_conversation_packet(
        &mut self,
        update: ConversationPacketUpdate,
    ) -> Option<Arc<ConversationPacket>> {
        match update {
            ConversationPacketUpdate::Captured(capture)
                if capture.reset_version == self.reset_version =>
            {
                self.conversation_packet = capture.packet;
            }
            ConversationPacketUpdate::Captured(_) | ConversationPacketUpdate::CarryForward => {}
            ConversationPacketUpdate::Clear => self.conversation_packet = None,
        }
        self.conversation_packet.clone()
    }
}

#[cfg(test)]
#[path = "history_conversation_packet_tests.rs"]
mod tests;
