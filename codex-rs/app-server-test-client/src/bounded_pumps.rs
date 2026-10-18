//! Inactive task ownership, not native-pipe cancellation or process containment.
//! Completion describes wrapper lifecycle only, never transport success.

use std::future::Future;
use std::future::poll_fn;
use std::task::Context;
use std::task::Poll;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::time::timeout_at;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskOutcome {
    Completed,
    Cancelled,
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PumpError {
    NotStarted,
    Deadline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PumpSnapshot {
    pub(super) stdout: Option<TaskOutcome>,
    pub(super) stderr: Option<TaskOutcome>,
}

enum Slot {
    Running(JoinHandle<()>),
    Joined(TaskOutcome),
}

pub(super) struct OwnedPumps {
    stdout: Slot,
    stderr: Slot,
    end: Option<Instant>,
    expired: bool,
}

impl OwnedPumps {
    pub(super) fn spawn<O, E>(stdout: O, stderr: E) -> Self
    where
        O: Future<Output = ()> + Send + 'static,
        E: Future<Output = ()> + Send + 'static,
    {
        Self {
            stdout: Slot::Running(tokio::spawn(stdout)),
            stderr: Slot::Running(tokio::spawn(stderr)),
            end: None,
            expired: false,
        }
    }

    fn abort(&self) {
        for slot in [&self.stdout, &self.stderr] {
            if let Slot::Running(task) = slot {
                task.abort();
            }
        }
    }

    /// Abort is a nonblocking signal, issued even after expiry. No joining or
    /// resource-release success is implied, even if join is never polled.
    pub(super) fn request_shutdown(&mut self, end: Instant) {
        self.end.get_or_insert(end);
        self.abort();
    }

    fn harvest(&mut self, cx: &mut Context<'_>) {
        for slot in [&mut self.stdout, &mut self.stderr] {
            if let Slot::Running(task) = slot
                && let Poll::Ready(result) = std::pin::Pin::new(task).poll(cx)
            {
                *slot = Slot::Joined(match result {
                    Ok(()) => TaskOutcome::Completed,
                    Err(error) if error.is_cancelled() => TaskOutcome::Cancelled,
                    Err(_) => TaskOutcome::Panicked,
                });
            }
        }
    }

    /// Facts can be observed after expiry, but never restore deadline success.
    pub(super) fn snapshot(&self) -> PumpSnapshot {
        let outcome = |slot: &Slot| match slot {
            Slot::Running(_) => None,
            Slot::Joined(outcome) => Some(*outcome),
        };
        PumpSnapshot {
            stdout: outcome(&self.stdout),
            stderr: outcome(&self.stderr),
        }
    }

    /// Borrowed handles never move into a disposable timeout future. Each pass
    /// polls each unjoined handle once and commits facts before checking time.
    pub(super) async fn join(&mut self) -> Result<PumpSnapshot, PumpError> {
        let end = self.end.ok_or(PumpError::NotStarted)?;
        let result = timeout_at(
            end,
            poll_fn(|cx| {
                self.harvest(cx);
                self.expired |= Instant::now() >= end;
                if self.expired {
                    return Poll::Ready(Err(PumpError::Deadline));
                }
                let snapshot = self.snapshot();
                if snapshot.stdout.is_some() && snapshot.stderr.is_some() {
                    Poll::Ready(Ok(snapshot))
                } else {
                    Poll::Pending
                }
            }),
        )
        .await;
        match result {
            Ok(result) => result,
            Err(_) => {
                self.expired = true;
                poll_fn(|cx| {
                    self.harvest(cx);
                    Poll::Ready(())
                })
                .await;
                Err(PumpError::Deadline)
            }
        }
    }
}

impl Drop for OwnedPumps {
    fn drop(&mut self) {
        // Runtime progress is required; dropping handles may detach unfinished
        // tasks even after abort. Only explicit joining proves destruction.
        self.abort();
    }
}

#[cfg(test)]
#[path = "bounded_pumps_tests.rs"]
mod tests;
