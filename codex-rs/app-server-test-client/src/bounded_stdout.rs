//! Inactive stdout-only admission, not a transport or task/process owner.
//! Compact queued and delivered payloads share 16 MiB and 64 message leases.
//! Reader staging/compaction can add approximately 16 MiB plus fixed buffers;
//! these are requested-payload bounds, not RSS limits. Later outbound integration
//! must share the transport budget, not introduce another independent allowance.

use super::bounded_frame_io::FrameError;
use super::bounded_frame_io::FrameReader;
use std::future::Future;
use std::sync::Arc;
use tokio::io::AsyncRead;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio::time::sleep_until;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_MESSAGES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StdoutError {
    Deadline,
    Frame(FrameError),
    ByteOverflow,
    MessageOverflow,
    PumpStopped,
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

pub(super) struct StdoutQueue {
    frames: mpsc::Receiver<LeasedFrame>,
    ending: oneshot::Receiver<Result<(), StdoutError>>,
    outcome: Option<Result<(), StdoutError>>,
    messages_closed: bool,
    finished: bool,
    pump_end: Instant,
}

impl StdoutQueue {
    fn fail(&mut self, error: StdoutError) -> StdoutError {
        self.outcome = Some(Err(error));
        self.frames.close();
        while self.frames.try_recv().is_ok() {}
        error
    }

    fn check(&mut self, end: Instant) -> Result<(), StdoutError> {
        if self.outcome.is_none() {
            self.outcome = match self.ending.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(oneshot::error::TryRecvError::Empty) => None,
                Err(oneshot::error::TryRecvError::Closed) => Some(Err(StdoutError::PumpStopped)),
            };
        }
        if let Some(Err(error)) = self.outcome {
            return Err(self.fail(error));
        }
        if Instant::now() >= end {
            return Err(self.fail(StdoutError::Deadline));
        }
        Ok(())
    }

    /// Pending cancellation preserves messages. Expiry seals this queue closed.
    /// No suspension occurs between dequeue, final checks and frame delivery.
    pub(super) async fn receive(
        &mut self,
        end: Instant,
    ) -> Result<Option<LeasedFrame>, StdoutError> {
        if self.finished {
            return Ok(None);
        }
        let end = end.min(self.pump_end);
        loop {
            self.check(end)?;
            if self.messages_closed && self.outcome == Some(Ok(())) {
                self.finished = true;
                return Ok(None);
            }
            tokio::select! {
                biased;
                outcome = &mut self.ending, if self.outcome.is_none() => {
                    self.outcome = Some(outcome.unwrap_or(Err(StdoutError::PumpStopped)));
                }
                frame = self.frames.recv(), if !self.messages_closed => {
                    self.check(end)?;
                    if let Some(frame) = frame {
                        return Ok(Some(frame));
                    }
                    self.messages_closed = true;
                }
                _ = sleep_until(end) => return Err(self.fail(StdoutError::Deadline)),
            }
        }
    }
}

/// The returned future already owns all streams/senders and must be driven by
/// its caller. Dropping the queue wakes a cooperatively polled blocked pump;
/// retaining an unpolled pump provides no cleanup guarantee.
pub(super) fn stdout_queue<R>(
    input: R,
    pump_end: Instant,
) -> (StdoutQueue, impl Future<Output = ()> + Send + 'static)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let (sender, frames) = mpsc::channel(MAX_MESSAGES);
    let (done, ending) = oneshot::channel();
    let messages = Arc::new(Semaphore::new(MAX_MESSAGES));
    let bytes = Arc::new(Semaphore::new(MAX_BYTES));
    let mut reader = FrameReader::new(input);
    let pump = async move {
        let outcome = async {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = sender.closed() => return Ok(()),
                    frame = reader.receive(pump_end) => frame.map_err(|error| match error {
                        FrameError::Deadline => StdoutError::Deadline,
                        other => StdoutError::Frame(other),
                    })?,
                };
                let Some(frame) = frame else { return Ok(()) };
                if Instant::now() >= pump_end {
                    return Err(StdoutError::Deadline);
                }
                let message = Arc::clone(&messages)
                    .try_acquire_owned()
                    .map_err(|_| StdoutError::MessageOverflow)?;
                let count = frame
                    .len()
                    .try_into()
                    .map_err(|_| StdoutError::ByteOverflow)?;
                let permit = Arc::clone(&bytes)
                    .try_acquire_many_owned(count)
                    .map_err(|_| StdoutError::ByteOverflow)?;
                if Instant::now() >= pump_end {
                    return Err(StdoutError::Deadline);
                }
                match sender.try_send(LeasedFrame {
                    bytes: frame,
                    _message: message,
                    _bytes: permit,
                }) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Closed(_)) => return Ok(()),
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        return Err(StdoutError::MessageOverflow);
                    }
                }
                tokio::task::yield_now().await;
            }
        }
        .await;
        let _ = done.send(outcome);
        drop(sender);
    };
    (
        StdoutQueue {
            frames,
            ending,
            outcome: None,
            messages_closed: false,
            finished: false,
            pump_end,
        },
        pump,
    )
}

#[cfg(test)]
#[path = "bounded_stdout_tests.rs"]
mod tests;
