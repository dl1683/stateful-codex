//! Inactive direct-child ownership, not descendant, pipe or task containment.
//! Native calls and scheduling are cooperative, not preempted by the deadline.

use std::process::Child;
use std::process::ExitStatus;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::sleep_until;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChildError {
    NotStarted,
    Deadline,
    TerminateIo,
    WaitIo,
}

pub(super) struct OwnedChild {
    child: Child,
    end: Option<Instant>,
    request: Option<Result<(), ChildError>>,
    attempted: bool,
    status: Option<ExitStatus>,
    expired: bool,
}

impl OwnedChild {
    pub(super) fn new(child: Child) -> Self {
        Self {
            child,
            end: None,
            request: None,
            attempted: false,
            status: None,
            expired: false,
        }
    }

    fn check(&mut self) -> Result<Instant, ChildError> {
        let end = self.end.ok_or(ChildError::NotStarted)?;
        if self.expired || Instant::now() >= end {
            self.expired = true;
            return Err(ChildError::Deadline);
        }
        Ok(end)
    }

    fn observe(&mut self) -> Result<(), ChildError> {
        self.check()?;
        let result = self.child.try_wait();
        if let Ok(Some(status)) = result {
            self.status = Some(status);
        }
        self.check()?;
        result.map(|_| ()).map_err(|_| ChildError::WaitIo)
    }

    /// The caller supplies the existing shutdown end, never a renewed allowance.
    /// Effects happen synchronously, including when reap is never polled.
    pub(super) fn request_shutdown(&mut self, end: Instant) -> Result<(), ChildError> {
        self.end.get_or_insert(end);
        self.check()?;
        if let Some(result) = self.request {
            return result;
        }
        let result = (|| {
            drop(self.child.stdin.take());
            self.check()?;
            self.observe()?;
            if self.status.is_some() {
                return Ok(());
            }
            self.check()?;
            self.attempted = true;
            let result = self.child.kill();
            self.check()?;
            if result.is_err() {
                self.observe()?;
                if self.status.is_none() {
                    return Err(ChildError::TerminateIo);
                }
            }
            Ok(())
        })();
        self.request = Some(result);
        result
    }

    /// Dropping this borrowed wait retains the child and first end. Only an
    /// observed status proves reaping; termination success alone never does.
    pub(super) async fn reap(&mut self) -> Result<ExitStatus, ChildError> {
        loop {
            let end = self.check()?;
            if let Some(status) = self.status {
                self.check()?;
                return Ok(status);
            }
            sleep_until((Instant::now() + Duration::from_millis(/*millis*/ 10)).min(end)).await;
            self.observe()?;
        }
    }

    /// A recorded fact remains available even after the allowance expires.
    pub(super) fn reaped(&self) -> Option<ExitStatus> {
        self.status
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        if self.status.is_none() && !self.attempted {
            self.attempted = true;
            let _ = self.child.kill();
        }
        // No blocking wait or guaranteed reaping, including after failed kill.
    }
}

#[cfg(test)]
#[path = "bounded_child_tests.rs"]
mod tests;
