use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Cooperative stop signal for one index operation.
///
/// A blocking scan cannot be aborted by dropping the future that awaits it, so the
/// scanner checks this signal between files and publication checks it before each
/// bounded transaction. A cancelled operation publishes nothing further and returns
/// [`super::ProjectIndexerError::Cancelled`]; units already committed stay committed.
#[derive(Clone, Debug, Default)]
pub struct IndexCancellation(Arc<AtomicBool>);

impl IndexCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(super) fn check(&self) -> Result<(), super::ProjectIndexerError> {
        if self.is_cancelled() {
            return Err(super::ProjectIndexerError::Cancelled);
        }
        Ok(())
    }
}
