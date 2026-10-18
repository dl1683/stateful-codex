//! Inactive deadline bookkeeping, not native cancellation or child ownership.
//! Callers acknowledge correlated completions explicitly; notifications never renew ends.

use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::timeout_at;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Initialization,
    Rpc,
    Turn,
    Compaction,
}

#[derive(Clone, Copy)]
pub(super) enum Scope {
    Phase,
    Rpc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeadlineError {
    InvalidState,
    InitializationExpired,
    RpcExpired,
    TurnExpired,
    CompactionExpired,
    TotalExpired,
}

enum Work {
    Uninitialized,
    Idle,
    Active {
        phase: Phase,
        end: Instant,
        rpc_end: Option<Instant>,
    },
    Failed(DeadlineError),
}

pub(super) struct JourneyDeadlines {
    work_end: Instant,
    work: Work,
}

impl JourneyDeadlines {
    pub(super) fn new() -> Self {
        Self {
            work_end: Instant::now() + Duration::from_secs(/*secs*/ 900),
            work: Work::Uninitialized,
        }
    }

    fn work_limit(&self) -> Instant {
        match &self.work {
            Work::Active { end, rpc_end, .. } => {
                self.work_end.min(*end).min(rpc_end.unwrap_or(*end))
            }
            Work::Uninitialized | Work::Idle | Work::Failed(_) => self.work_end,
        }
    }

    fn check_work(&mut self) -> Result<(), DeadlineError> {
        let now = Instant::now();
        let error = match &self.work {
            Work::Failed(error) => Some(*error),
            Work::Uninitialized | Work::Idle | Work::Active { .. } if now >= self.work_end => {
                Some(DeadlineError::TotalExpired)
            }
            Work::Active { phase, end, .. } if now >= *end => Some(match phase {
                Phase::Initialization => DeadlineError::InitializationExpired,
                Phase::Rpc => DeadlineError::RpcExpired,
                Phase::Turn => DeadlineError::TurnExpired,
                Phase::Compaction => DeadlineError::CompactionExpired,
            }),
            Work::Active {
                rpc_end: Some(end), ..
            } if now >= *end => Some(DeadlineError::RpcExpired),
            Work::Uninitialized | Work::Idle | Work::Active { .. } => None,
        };
        if let Some(error) = error {
            self.work = Work::Failed(error);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn begin_phase(&mut self, phase: Phase) -> Result<(), DeadlineError> {
        self.check_work()?;
        if !matches!(
            (&self.work, phase),
            (Work::Uninitialized, Phase::Initialization)
                | (Work::Idle, Phase::Rpc | Phase::Turn | Phase::Compaction)
        ) {
            return Err(DeadlineError::InvalidState);
        }
        let seconds = match phase {
            Phase::Initialization => 60,
            Phase::Rpc => 30,
            Phase::Turn | Phase::Compaction => 180,
        };
        self.work = Work::Active {
            phase,
            end: (Instant::now() + Duration::from_secs(seconds)).min(self.work_end),
            rpc_end: None,
        };
        Ok(())
    }

    pub(super) fn begin_rpc(&mut self) -> Result<(), DeadlineError> {
        self.check_work()?;
        if let Work::Active {
            phase: Phase::Initialization | Phase::Turn | Phase::Compaction,
            end,
            rpc_end,
        } = &mut self.work
            && rpc_end.is_none()
        {
            *rpc_end = Some((Instant::now() + Duration::from_secs(/*secs*/ 30)).min(*end));
            return Ok(());
        }
        Err(DeadlineError::InvalidState)
    }

    /// Snapshots are not revocable transport capabilities. Both work scopes
    /// include an earlier active child; Scope::Rpc names only the nested slot.
    pub(super) fn end(&mut self, scope: Scope) -> Result<Instant, DeadlineError> {
        self.check_work()?;
        if matches!(
            (&self.work, scope),
            (Work::Active { .. }, Scope::Phase)
                | (
                    Work::Active {
                        rpc_end: Some(_),
                        ..
                    },
                    Scope::Rpc
                )
        ) {
            Ok(self.work_limit())
        } else {
            Err(DeadlineError::InvalidState)
        }
    }

    /// The owned future is checked around every poll, including Pending polls.
    /// Rejected Ready outputs are dropped before returning; waits never finish slots.
    pub(super) async fn wait<F: Future>(
        &mut self,
        scope: Scope,
        future: F,
    ) -> Result<F::Output, DeadlineError> {
        let end = self.end(scope)?;
        tokio::pin!(future);
        let checked = poll_fn(|cx| {
            if let Err(error) = self.end(scope) {
                return Poll::Ready(Err(error));
            }
            let result = future.as_mut().poll(cx);
            match self.end(scope) {
                Ok(_) => result.map(Ok),
                Err(error) => {
                    drop(result);
                    Poll::Ready(Err(error))
                }
            }
        });
        match timeout_at(end, checked).await {
            Ok(result) => result,
            Err(_) => {
                self.check_work()?;
                Err(DeadlineError::InvalidState)
            }
        }
    }

    pub(super) fn finish(&mut self, scope: Scope) -> Result<(), DeadlineError> {
        self.end(scope)?;
        match scope {
            Scope::Rpc => {
                if let Work::Active { rpc_end, .. } = &mut self.work {
                    *rpc_end = None;
                }
            }
            Scope::Phase => {
                if let Work::Active {
                    rpc_end: Some(_), ..
                } = self.work
                {
                    return Err(DeadlineError::InvalidState);
                }
                self.work = Work::Idle;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "bounded_deadlines_tests.rs"]
mod tests;
