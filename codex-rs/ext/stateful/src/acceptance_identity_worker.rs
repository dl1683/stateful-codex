//! A reader slot lasts for the worker's lifetime, including after an async timeout or panic.
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use crate::acceptance_identity_reason::IdentityGap;
use crate::acceptance_observation::MAX_OUTSTANDING_READERS;
use crate::acceptance_observation::OUTSTANDING_READERS;

struct ReaderSlot;

impl Drop for ReaderSlot {
    fn drop(&mut self) {
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(super) async fn read_identity<T: Send + 'static>(
    budget: Duration,
    read: impl FnOnce(Instant) -> Result<T, IdentityGap> + Send + 'static,
) -> Result<T, IdentityGap> {
    if OUTSTANDING_READERS.fetch_add(1, Ordering::SeqCst) >= MAX_OUTSTANDING_READERS {
        OUTSTANDING_READERS.fetch_sub(1, Ordering::SeqCst);
        return Err(IdentityGap::Slots);
    }
    let slot = ReaderSlot;
    let deadline = Instant::now() + budget;
    let worker = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        if Instant::now() >= deadline { return Err(IdentityGap::Time); }
        read(deadline)
    });
    tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), worker)
        .await
        .map_err(|_| IdentityGap::Time)?
        .map_err(|_| IdentityGap::Worker)?
}
