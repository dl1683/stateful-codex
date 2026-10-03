use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Cooperative stop signal for one index operation.
///
/// A blocking scan cannot be aborted by dropping the future that awaits it, so the
/// scanner checks this signal before each read unit and every publication transaction
/// checks it once it holds the writer lock. A transaction that passed that check before
/// the cancel may still commit its one bounded unit; after that, the operation publishes
/// nothing further and returns [`super::ProjectIndexerError::Cancelled`]. Units already
/// committed stay committed.
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
