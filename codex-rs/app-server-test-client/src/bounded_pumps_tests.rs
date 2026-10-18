use super::OwnedPumps;
use super::PumpError;
use super::PumpSnapshot;
use super::TaskOutcome::Cancelled;
use super::TaskOutcome::Completed;
use super::TaskOutcome::Panicked;
use crate::bounded_deadlines::JourneyDeadlines;
use crate::bounded_deadlines::Scope;
use crate::bounded_frame_io::FrameError;
use crate::bounded_payload::MAX_BYTES;
use crate::bounded_payload::PayloadBudget;
use crate::bounded_stderr::StderrDrain;
use crate::bounded_stderr::StderrStop;
use crate::bounded_stdout::StdoutError;
use crate::bounded_stdout::stdout_queue;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::pending;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadBuf;
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio::time::timeout;

struct Signal(Option<oneshot::Sender<()>>);

impl Drop for Signal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

fn signal() -> (Signal, oneshot::Receiver<()>) {
    let (sender, receiver) = oneshot::channel();
    (Signal(Some(sender)), receiver)
}

async fn parked(signal: Signal) {
    let _signal = signal;
    pending::<()>().await;
}

async fn destroyed(receiver: oneshot::Receiver<()>) {
    timeout(Duration::from_secs(/*secs*/ 1), receiver)
        .await
        .unwrap()
        .unwrap();
}

const CANCELLED: PumpSnapshot = PumpSnapshot {
    stdout: Some(Cancelled),
    stderr: Some(Cancelled),
};

#[tokio::test(flavor = "current_thread")]
async fn unpolled_and_pending_waits_keep_handles_and_task_owned_resources() {
    let budget = PayloadBudget::new();
    let lease = budget
        .try_admit(vec![0; MAX_BYTES].into_boxed_slice())
        .unwrap();
    let (input, mut peer) = tokio::io::duplex(/*max_buf_size*/ 1);
    let mut owner = OwnedPumps::spawn(
        async move {
            let _lease = lease;
            pending::<()>().await;
        },
        async move {
            let _input = input;
            pending::<()>().await;
        },
    );
    assert_eq!(owner.join().await, Err(PumpError::NotStarted));
    owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
    drop(owner.join());
    {
        let wait = owner.join();
        tokio::pin!(wait);
        poll_fn(|cx| {
            assert!(wait.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert_eq!(
        owner.snapshot(),
        PumpSnapshot {
            stdout: None,
            stderr: None
        }
    );
    assert_eq!(owner.join().await, Ok(CANCELLED));
    assert!(
        budget
            .try_admit(vec![0; MAX_BYTES].into_boxed_slice())
            .is_ok()
    );
    assert!(peer.write_all(b"x").await.is_err());
    assert_eq!(owner.join().await, Ok(CANCELLED));
}

#[tokio::test(flavor = "current_thread")]
async fn partial_join_facts_survive_cancellation_for_both_identities() {
    for stdout_first in [true, false] {
        let (finished, done) = signal();
        let completes = async move {
            drop(finished);
        };
        let mut owner = if stdout_first {
            OwnedPumps::spawn(completes, pending())
        } else {
            OwnedPumps::spawn(pending(), completes)
        };
        destroyed(done).await;
        owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
        {
            let wait = owner.join();
            tokio::pin!(wait);
            poll_fn(|cx| {
                assert!(wait.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        }
        let expected = if stdout_first {
            PumpSnapshot {
                stdout: Some(Completed),
                stderr: None,
            }
        } else {
            PumpSnapshot {
                stdout: None,
                stderr: Some(Completed),
            }
        };
        assert_eq!(owner.snapshot(), expected);
        let finished = if stdout_first {
            PumpSnapshot {
                stderr: Some(Cancelled),
                ..expected
            }
        } else {
            PumpSnapshot {
                stdout: Some(Cancelled),
                ..expected
            }
        };
        assert_eq!(owner.join().await, Ok(finished));
        assert_eq!(owner.join().await, Ok(finished));
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn expired_first_request_still_aborts_and_later_harvest_never_succeeds() {
    let mut clock = JourneyDeadlines::new();
    clock.begin_shutdown().unwrap();
    let end = clock.end(Scope::Shutdown).unwrap();
    tokio::time::advance(Duration::from_secs(/*secs*/ 5)).await;
    let (left, left_done) = signal();
    let (right, right_done) = signal();
    let mut owner = OwnedPumps::spawn(parked(left), parked(right));
    owner.request_shutdown(end);
    assert_eq!(owner.join().await, Err(PumpError::Deadline));
    assert_eq!(
        owner.snapshot(),
        PumpSnapshot {
            stdout: None,
            stderr: None
        }
    );
    destroyed(left_done).await;
    destroyed(right_done).await;
    owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 50));
    assert_eq!(owner.join().await, Err(PumpError::Deadline));
    assert_eq!(owner.snapshot(), CANCELLED);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn later_request_does_not_renew_end_and_ready_joins_at_exact_end_refuse() {
    let mut clock = JourneyDeadlines::new();
    clock.begin_shutdown().unwrap();
    let end = clock.end(Scope::Shutdown).unwrap();
    let (left, left_done) = signal();
    let (right, right_done) = signal();
    let mut owner = OwnedPumps::spawn(
        async move {
            drop(left);
        },
        async move {
            drop(right);
        },
    );
    destroyed(left_done).await;
    destroyed(right_done).await;
    owner.request_shutdown(end);
    owner.request_shutdown(end + Duration::from_secs(/*secs*/ 50));
    tokio::time::advance(Duration::from_secs(/*secs*/ 5)).await;
    assert_eq!(
        (owner.end, owner.join().await),
        (Some(end), Err(PumpError::Deadline))
    );
    assert_eq!(
        owner.snapshot(),
        PumpSnapshot {
            stdout: Some(Completed),
            stderr: Some(Completed)
        }
    );
}

#[tokio::test(flavor = "current_thread")]
async fn completed_and_panicked_task_identities_are_preserved() {
    for stdout_completed in [true, false] {
        let (left, left_done) = signal();
        let (right, right_done) = signal();
        let completes = async move {
            drop(left);
        };
        let panics = async move {
            let _right = right;
            panic!("synthetic pump panic");
        };
        let mut owner = if stdout_completed {
            OwnedPumps::spawn(completes, panics)
        } else {
            OwnedPumps::spawn(panics, completes)
        };
        destroyed(left_done).await;
        destroyed(right_done).await;
        owner.request_shutdown(Instant::now() + Duration::from_secs(/*secs*/ 5));
        let expected = if stdout_completed {
            PumpSnapshot {
                stdout: Some(Completed),
                stderr: Some(Panicked),
            }
        } else {
            PumpSnapshot {
                stdout: Some(Panicked),
                stderr: Some(Completed),
            }
        };
        assert_eq!(owner.join().await, Ok(expected));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn drop_requests_abort_but_destruction_requires_runtime_progress() {
    let (left, left_done) = signal();
    let (right, right_done) = signal();
    let owner = OwnedPumps::spawn(parked(left), parked(right));
    drop(owner);
    destroyed(left_done).await;
    destroyed(right_done).await;
}

struct Fails;

impl AsyncRead for Fails {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Err(std::io::Error::other("synthetic read failure")))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn wrapper_completion_does_not_mean_transport_success() {
    let end = Instant::now() + Duration::from_secs(/*secs*/ 5);
    let (mut queue, stdout) = stdout_queue(&b"partial"[..], end, PayloadBudget::new());
    let (left, left_done) = signal();
    let (right, right_done) = signal();
    let (sender, stop) = oneshot::channel();
    let mut owner = OwnedPumps::spawn(
        async move {
            stdout.await;
            drop(left);
        },
        async move {
            let _right = right;
            let result = StderrDrain::new(Fails, end).drain().await;
            let _ = sender.send(result);
        },
    );
    destroyed(left_done).await;
    destroyed(right_done).await;
    owner.request_shutdown(end);
    assert_eq!(
        owner.join().await,
        Ok(PumpSnapshot {
            stdout: Some(Completed),
            stderr: Some(Completed)
        })
    );
    assert_eq!(
        queue.receive(end).await.map(|_| ()),
        Err(StdoutError::Frame(FrameError::PartialEof))
    );
    assert_eq!(stop.await.unwrap(), StderrStop::Io);
}
