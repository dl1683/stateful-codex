//! Inactive LF byte framing only. Queue budgets, stderr and native child ownership
//! must be qualified separately before a controller can use this component.

use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::time::Instant;
use tokio::time::timeout_at;

const MAX_FRAME: usize = 8 * 1024 * 1024;
const CHUNK: usize = 8192;

/// Errors deliberately contain no payload, parser text or underlying I/O diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameError {
    Deadline,
    TooLarge,
    EmbeddedNewline,
    PartialEof,
    Io,
    Allocation,
    Poisoned,
}

fn check_deadline(end: Instant) -> Result<(), FrameError> {
    if Instant::now() >= end {
        Err(FrameError::Deadline)
    } else {
        Ok(())
    }
}

pub(super) struct FrameReader<R> {
    input: BufReader<R>,
    partial: Vec<u8>,
    complete: bool,
    closed: bool,
    poisoned: bool,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub(super) fn new(input: R) -> Self {
        Self {
            input: BufReader::with_capacity(CHUNK, input),
            partial: Vec::new(),
            complete: false,
            closed: false,
            poisoned: false,
        }
    }

    /// Cancellation retains partial bytes, including an already consumed delimiter.
    /// The caller supplies the same immutable operation end across notifications.
    pub(super) async fn receive(&mut self, end: Instant) -> Result<Option<Box<[u8]>>, FrameError> {
        if self.poisoned {
            return Err(FrameError::Poisoned);
        }
        let result = timeout_at(end, async {
            loop {
                check_deadline(end)?;
                if self.complete {
                    self.complete = false;
                    return Ok(Some(std::mem::take(&mut self.partial).into_boxed_slice()));
                }
                if self.closed {
                    return Ok(None);
                }
                let available = self.input.fill_buf().await.map_err(|_| FrameError::Io)?;
                check_deadline(end)?;
                if available.is_empty() {
                    if !self.partial.is_empty() {
                        return Err(FrameError::PartialEof);
                    }
                    self.closed = true;
                    continue;
                }
                let delimiter = available.iter().position(|byte| *byte == b'\n');
                let count = delimiter.unwrap_or(available.len());
                if count > MAX_FRAME - self.partial.len() {
                    return Err(FrameError::TooLarge);
                }
                let needed = self.partial.len() + count;
                if needed > self.partial.capacity() {
                    let capacity = self
                        .partial
                        .capacity()
                        .saturating_mul(2)
                        .max(CHUNK)
                        .max(needed)
                        .min(MAX_FRAME);
                    self.partial
                        .try_reserve_exact(capacity - self.partial.len())
                        .map_err(|_| FrameError::Allocation)?;
                }
                self.partial.extend_from_slice(&available[..count]);
                self.complete = delimiter.is_some();
                self.input.consume(count + usize::from(self.complete));
                // Yield even when input is continuously ready, retaining all read state.
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or(Err(FrameError::Deadline));
        if let Err(error) = result
            && error != FrameError::Deadline
        {
            self.poisoned = true;
        }
        result
    }
}

pub(super) struct FrameWriter<W> {
    output: W,
    poisoned: bool,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub(super) fn new(output: W) -> Self {
        Self {
            output,
            poisoned: false,
        }
    }

    /// Any failed or dropped write permanently poisons this stream. Never replay
    /// a whole frame after a partial write, even if its new deadline would allow it.
    pub(super) async fn send(&mut self, frame: &[u8], end: Instant) -> Result<(), FrameError> {
        if self.poisoned {
            return Err(FrameError::Poisoned);
        }
        check_deadline(end)?;
        if frame.len() > MAX_FRAME {
            return Err(FrameError::TooLarge);
        }
        if frame.contains(&b'\n') {
            return Err(FrameError::EmbeddedNewline);
        }
        check_deadline(end)?;
        self.poisoned = true;
        let result = timeout_at(end, async {
            for chunk in frame.chunks(CHUNK) {
                check_deadline(end)?;
                self.output
                    .write_all(chunk)
                    .await
                    .map_err(|_| FrameError::Io)?;
                tokio::task::yield_now().await;
            }
            check_deadline(end)?;
            self.output
                .write_all(b"\n")
                .await
                .map_err(|_| FrameError::Io)?;
            self.output.flush().await.map_err(|_| FrameError::Io)?;
            check_deadline(end)
        })
        .await
        .unwrap_or(Err(FrameError::Deadline));
        if result.is_ok() {
            self.poisoned = false;
        }
        result
    }
}

#[cfg(test)]
#[path = "bounded_frame_io_tests.rs"]
mod tests;
