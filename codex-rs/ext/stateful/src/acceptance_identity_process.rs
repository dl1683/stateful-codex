//! Streamed queries supervised by an owner of the child, with no shared child lock.
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::acceptance_identity_reason::Bound;
use crate::acceptance_identity_reason::IdentityGap;

#[cfg(test)]
#[path = "acceptance_identity_process_tests.rs"]
mod tests;

pub(super) fn records(
    mut command: Command,
    terminator: u8,
    output_limit: u64,
    deadline: Instant,
    mut on_record: impl FnMut(&[u8]) -> Result<(), IdentityGap>,
) -> Result<i32, IdentityGap> {
    if Instant::now() >= deadline {
        return Err(IdentityGap::Time);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| IdentityGap::Git)?;
    let stdout = child.stdout.take();
    let (cancel, cancelled) = mpsc::channel();
    let supervisor = thread::spawn(move || {
        let mut stopping = false;
        loop {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(IdentityGap::Time);
            }
            if !stopping && cancelled.try_recv() != Err(mpsc::TryRecvError::Empty) {
                let _ = child.kill();
                stopping = true;
            }
            match child.try_wait() {
                Ok(Some(status)) => return status.code().ok_or(IdentityGap::Git),
                Ok(None) => {}
                Err(_) => {
                    let _ = child.kill();
                    // Poll again rather than blocking on a wait before the deadline.
                    stopping = true;
                }
            }
            thread::sleep(
                Duration::from_millis(/*millis*/ 10)
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    });
    let (sender, receiver) = mpsc::sync_channel(/*bound*/ 1);
    let reader = thread::spawn(move || {
        let Some(stdout) = stdout else {
            let _ = sender.send(Err(IdentityGap::Git));
            return;
        };
        let mut reader = BufReader::new(stdout.take(output_limit.saturating_add(/*rhs*/ 1)));
        let mut total = 0_u64;
        loop {
            let mut record = Vec::new();
            let result = match reader.read_until(terminator, &mut record) {
                Ok(0) => Ok(None),
                Ok(read) => {
                    total += read as u64;
                    if total > output_limit {
                        Err(IdentityGap::Bound(Bound::GitOutput))
                    } else {
                        if record.last() == Some(&terminator) {
                            record.pop();
                        }
                        Ok(Some(record))
                    }
                }
                Err(_) => Err(IdentityGap::Git),
            };
            let done = !matches!(result, Ok(Some(_)));
            if sender.send(result).is_err() || done {
                break;
            }
        }
    });
    let streamed = (|| loop {
        if Instant::now() >= deadline {
            return Err(IdentityGap::Time);
        }
        let result = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => IdentityGap::Time,
                mpsc::RecvTimeoutError::Disconnected => IdentityGap::Worker,
            })?;
        match result? {
            None => return Ok(()),
            Some(record) => {
                if Instant::now() >= deadline {
                    return Err(IdentityGap::Time);
                }
                on_record(&record)?;
            }
        }
    })();
    if streamed.is_err() {
        let _ = cancel.send(());
    }
    let exited = supervisor.join().map_err(|_| IdentityGap::Worker)?;
    // An inherited pipe can outlive the child. Do not wait for that reader after a gap.
    drop(receiver);
    if reader.is_finished() {
        let _ = reader.join();
    }
    streamed?;
    exited
}
