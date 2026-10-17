//! Fixed-size response ordering only, without OS trust or lifecycle authority.

use thiserror::Error;

use crate::FailureStage;
use crate::Response;
use crate::ResponseEvent;

#[derive(Clone, Copy)]
enum Phase {
    AwaitReady,
    AwaitSpawn,
    Running,
    RootObserved,
    Refused,
}

/// Parent-side ordering for one worker's unauthenticated assertions. The parent
/// supplies its actual worker PID and correlation nonce, never worker claims.
/// Root exit does not authorize preservation, termination, drain or completion.
/// No handles, process ownership or local lifecycle decisions are represented.
pub struct WorkerResponseSequence {
    nonce: [u8; 16],
    worker_pid: u32,
    phase: Phase,
}

/// Bounded sequence refusals. Error text never includes payload/host diagnostics.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum SequenceError {
    #[error("console worker identity refused")]
    WorkerIdentity,
    #[error("console response correlation refused")]
    Correlation,
    #[error("console response order refused")]
    Order,
    #[error("console worker reported failure")]
    WorkerFailed {
        stage: FailureStage,
        win32_code: u32,
    },
}

impl WorkerResponseSequence {
    /// Start one sequence using trusted parent bootstrap inputs. Correlation and
    /// PID equality are not authentication of the sender or proof of OS effects.
    pub fn new(nonce: [u8; 16], expected_worker_pid: u32) -> Result<Self, SequenceError> {
        if expected_worker_pid == 0 {
            return Err(SequenceError::WorkerIdentity);
        }
        Ok(Self {
            nonce,
            worker_pid: expected_worker_pid,
            phase: Phase::AwaitReady,
        })
    }

    /// Admit Ready -> Spawned -> RootExited, or record a reported failure.
    /// Any refusal permanently poisons this sequence; even a later valid event
    /// cannot recover it. Create a new sequence only for a new worker lifecycle.
    /// Failed after RootExited still refuses: root observation is not terminal.
    /// The caller must separately decode framing and own all actual OS admission,
    /// paired-job decisions, expected worker exit and drain/live-child accounting.
    pub fn observe(&mut self, response: &Response) -> Result<(), SequenceError> {
        let phase = std::mem::replace(&mut self.phase, Phase::Refused);
        if matches!(phase, Phase::Refused) {
            return Err(SequenceError::Order);
        }
        if response.nonce != self.nonce {
            return Err(SequenceError::Correlation);
        }
        match response.event {
            ResponseEvent::Ready { worker_pid } => {
                if !matches!(phase, Phase::AwaitReady) {
                    return Err(SequenceError::Order);
                }
                if worker_pid != self.worker_pid {
                    return Err(SequenceError::WorkerIdentity);
                }
                self.phase = Phase::AwaitSpawn;
            }
            ResponseEvent::Spawned { target_pid } => {
                if !matches!(phase, Phase::AwaitSpawn) {
                    return Err(SequenceError::Order);
                }
                if target_pid == 0 || target_pid == self.worker_pid {
                    return Err(SequenceError::WorkerIdentity);
                }
                self.phase = Phase::Running;
            }
            ResponseEvent::RootExited { exit_code: _ } => {
                if !matches!(phase, Phase::Running) {
                    return Err(SequenceError::Order);
                }
                self.phase = Phase::RootObserved;
            }
            ResponseEvent::Failed { stage, win32_code } => {
                return Err(SequenceError::WorkerFailed { stage, win32_code });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "sequence_tests.rs"]
mod tests;
