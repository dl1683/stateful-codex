//! Inactive caller-owned stderr drainage, not task/process ownership.
//! Retention never stops reads: only the latest 64 KiB remains, plus 8 KiB
//! scratch while draining. Reader storage and allocator/RSS costs are separate.
//! Dropping a borrowed drain future preserves committed bytes and the original
//! end. Dropping the collector drops R, not necessarily native I/O or a child.

use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::time::Instant;
use tokio::time::timeout_at;

const MAX_TAIL: usize = 64 * 1024;
const CHUNK: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StderrStop {
    Eof,
    Deadline,
    Io,
}

pub(super) struct StderrDrain<R> {
    reader: R,
    end: Instant,
    tail: [u8; MAX_TAIL],
    next: usize,
    len: usize,
    discarded: u64,
    stopped: Option<StderrStop>,
}

impl<R: AsyncRead + Unpin> StderrDrain<R> {
    pub(super) fn new(reader: R, end: Instant) -> Self {
        Self {
            reader,
            end,
            tail: [0; MAX_TAIL],
            next: 0,
            len: 0,
            discarded: 0,
            stopped: None,
        }
    }

    /// A pending read may resume at expiry. Commit returned bytes before the
    /// post-read deadline check; never suspend between read and retention.
    /// Only fixed stop kinds escape, never raw stderr or underlying errors.
    pub(super) async fn drain(&mut self) -> StderrStop {
        if let Some(stop) = self.stopped {
            return stop;
        }
        let end = self.end;
        let stop = timeout_at(end, async {
            let mut scratch = [0; CHUNK];
            loop {
                if Instant::now() >= end {
                    return StderrStop::Deadline;
                }
                let result = self.reader.read(&mut scratch).await;
                if let Ok(count) = &result {
                    let count = *count;
                    let evicted = count.saturating_sub(MAX_TAIL - self.len);
                    self.discarded = self.discarded.saturating_add(evicted as u64);
                    let first = count.min(MAX_TAIL - self.next);
                    self.tail[self.next..self.next + first].copy_from_slice(&scratch[..first]);
                    self.tail[..count - first].copy_from_slice(&scratch[first..count]);
                    self.next = (self.next + count) % MAX_TAIL;
                    self.len = (self.len + count).min(MAX_TAIL);
                }
                if Instant::now() >= end {
                    return StderrStop::Deadline;
                }
                match result {
                    Ok(0) => return StderrStop::Eof,
                    Ok(_) => tokio::task::yield_now().await,
                    Err(_) => return StderrStop::Io,
                }
            }
        })
        .await
        .unwrap_or(StderrStop::Deadline);
        self.stopped = Some(stop);
        stop
    }
}

#[cfg(test)]
#[path = "bounded_stderr_tests.rs"]
mod tests;
