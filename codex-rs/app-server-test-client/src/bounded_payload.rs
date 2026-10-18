//! Inactive shared admission for compact payloads, not outbound-write enforcement.
//! Each budget bounds admitted lengths, not caller input, serialization, reader
//! staging, compaction allocations, I/O buffering, allocator overhead or RSS.

use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;

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

#[cfg(test)]
#[path = "bounded_payload_tests.rs"]
mod tests;
