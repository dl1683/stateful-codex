//! Inactive shared admission and pool-bound typed writing, not a transport owner.
//! Each budget bounds admitted lengths, not caller input, serialization, reader
//! staging, compaction allocations, I/O buffering, allocator overhead or RSS.

use super::bounded_frame_io::FrameError;
use super::bounded_frame_io::FrameWriter;
use std::sync::Arc;
use tokio::io::AsyncWrite;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::time::Instant;

pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_MESSAGES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AdmissionError {
    MessageOverflow,
    ByteOverflow,
}

/// Clones share one immutable pair of allowances; new budgets are independent.
#[derive(Clone)]
pub(super) struct PayloadBudget {
    messages: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

pub(super) struct LeasedFrame {
    bytes: Box<[u8]>,
    _message: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

impl LeasedFrame {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl PayloadBudget {
    pub(super) fn new() -> Self {
        Self {
            messages: Arc::new(Semaphore::new(MAX_MESSAGES)),
            bytes: Arc::new(Semaphore::new(MAX_BYTES)),
        }
    }

    /// Consumes existing compact bytes without waiting, copying or validating
    /// framing. Refusal drops the box; empty frames still consume one message.
    pub(super) fn try_admit(&self, bytes: Box<[u8]>) -> Result<LeasedFrame, AdmissionError> {
        let message = Arc::clone(&self.messages)
            .try_acquire_owned()
            .map_err(|_| AdmissionError::MessageOverflow)?;
        let count = bytes
            .len()
            .try_into()
            .map_err(|_| AdmissionError::ByteOverflow)?;
        let permit = Arc::clone(&self.bytes)
            .try_acquire_many_owned(count)
            .map_err(|_| AdmissionError::ByteOverflow)?;
        Ok(LeasedFrame {
            bytes,
            _message: message,
            _bytes: permit,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SendError {
    ForeignBudget,
    Frame(FrameError),
}

pub(super) struct LeasedWriter<W> {
    inner: FrameWriter<W>,
    budget: PayloadBudget,
}

impl<W: AsyncWrite + Unpin> LeasedWriter<W> {
    pub(super) fn new(output: W, budget: PayloadBudget) -> Self {
        Self {
            inner: FrameWriter::new(output),
            budget,
        }
    }

    /// The future owns the whole lease even before polling, and through write/
    /// flush completion or destruction. No background expiry/progress is promised.
    pub(super) async fn send(&mut self, frame: LeasedFrame, end: Instant) -> Result<(), SendError> {
        if !Arc::ptr_eq(&self.budget.messages, frame._message.semaphore()) {
            drop(frame);
            return Err(SendError::ForeignBudget);
        }
        let result = self
            .inner
            .send(frame.bytes(), end)
            .await
            .map_err(SendError::Frame);
        drop(frame);
        result
    }
}

#[cfg(test)]
#[path = "bounded_payload_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "bounded_writer_tests.rs"]
mod writer_tests;
