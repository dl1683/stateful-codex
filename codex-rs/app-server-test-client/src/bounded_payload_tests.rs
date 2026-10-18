use super::*;
use crate::bounded_frame_io::FrameError;
use crate::bounded_stdout::StdoutError;
use crate::bounded_stdout::stdout_queue;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::ReadBuf;
use tokio::time::Instant;

fn end() -> Instant {
    Instant::now() + Duration::from_secs(/*secs*/ 30)
}

fn bytes(len: usize) -> Box<[u8]> {
    vec![b'x'; len].into_boxed_slice()
}

fn wire(lengths: &[usize]) -> io::Cursor<Vec<u8>> {
    let mut input = Vec::new();
    for len in lengths {
        input.resize(input.len() + len, b'x');
        input.push(b'\n');
    }
    io::Cursor::new(input)
}

async fn pending(mut future: Pin<&mut impl Future>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test]
async fn queued_and_delivered_stdout_compete_with_independent_ownership() {
    let budget = PayloadBudget::new();
    let independent = budget.try_admit(bytes(MAX_BYTES / 2)).unwrap();
    let (mut queue, pump) = stdout_queue(wire(&[MAX_BYTES / 2]), end(), budget.clone());
    pump.await;
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    let delivered = queue.receive(end()).await.unwrap().unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop(delivered);
    let replacement = budget.try_admit(bytes(MAX_BYTES / 2)).unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop((replacement, independent));
    assert!(budget.try_admit(bytes(MAX_BYTES)).is_ok());
}

#[tokio::test]
async fn independent_ownership_overflows_stdout_and_terminal_cleanup_keeps_delivered_credit() {
    let budget = PayloadBudget::new();
    let independent = budget.try_admit(bytes(MAX_BYTES / 2)).unwrap();
    let input = wire(&[MAX_BYTES / 4, MAX_BYTES / 4, 1]);
    let (mut queue, pump) = stdout_queue(input, end(), budget.clone());
    let (_, delivered) = tokio::join!(pump, async { queue.receive(end()).await.unwrap().unwrap() });
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    assert_eq!(
        queue.receive(end()).await.err(),
        Some(StdoutError::ByteOverflow)
    );
    let reclaimed = budget.try_admit(bytes(MAX_BYTES / 4)).unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop(delivered);
    let recovered = budget.try_admit(bytes(MAX_BYTES / 4)).unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop((reclaimed, recovered, independent));
    assert!(budget.try_admit(bytes(MAX_BYTES)).is_ok());
}

#[tokio::test]
async fn dropping_populated_queue_reclaims_only_queued_ownership() {
    let budget = PayloadBudget::new();
    let (mut queue, pump) =
        stdout_queue(wire(&[MAX_BYTES / 2, MAX_BYTES / 2]), end(), budget.clone());
    pump.await;
    let delivered = queue.receive(end()).await.unwrap().unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop(queue);
    let reclaimed = budget.try_admit(bytes(MAX_BYTES / 2)).unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop(delivered);
    let recovered = budget.try_admit(bytes(MAX_BYTES / 2)).unwrap();
    drop((reclaimed, recovered));
    assert!(budget.try_admit(bytes(MAX_BYTES)).is_ok());
}

#[test]
fn empty_messages_exhaust_credit_before_bytes_and_byte_refusal_rolls_back() {
    let budget = PayloadBudget::new();
    let full = budget.try_admit(bytes(MAX_BYTES)).unwrap();
    let mut empty: Vec<_> = (1..MAX_MESSAGES)
        .map(|_| budget.try_admit(Box::default()).unwrap())
        .collect();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::MessageOverflow)
    );
    drop(empty.pop());
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    let last = budget.try_admit(Box::default()).unwrap();
    assert_eq!(
        budget.try_admit(Box::default()).err(),
        Some(AdmissionError::MessageOverflow)
    );
    drop((full, empty, last));
    let recovered: Vec<_> = (0..MAX_MESSAGES)
        .map(|_| budget.try_admit(Box::default()).unwrap())
        .collect();
    assert_eq!(
        budget.try_admit(Box::default()).err(),
        Some(AdmissionError::MessageOverflow)
    );
    drop(recovered);
    assert!(budget.try_admit(bytes(MAX_BYTES)).is_ok());
}

#[test]
fn clones_share_credits_and_frames_outlive_handles_without_merging_separate_pools() {
    let original = PayloadBudget::new();
    let shared = original.clone();
    let held = original.try_admit(bytes(MAX_BYTES)).unwrap();
    drop(original);
    assert_eq!(
        shared.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    let separate = PayloadBudget::new();
    let other = separate.try_admit(bytes(MAX_BYTES)).unwrap();
    assert_eq!(
        separate.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop((separate, other));
    assert_eq!(
        shared.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop(held);
    assert!(shared.try_admit(bytes(MAX_BYTES)).is_ok());
}

struct Broken;
impl AsyncRead for Broken {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::other("PRIVATE_IO_SENTINEL")))
    }
}

#[tokio::test]
async fn frame_failures_release_queued_credits_without_payload_diagnostics() {
    let cases: Vec<(Box<dyn AsyncRead + Unpin + Send>, FrameError)> = vec![
        (
            Box::new(b"PRIVATE_PAYLOAD\npartial".as_slice()),
            FrameError::PartialEof,
        ),
        (
            Box::new(b"PRIVATE_PAYLOAD\n".as_slice().chain(Broken)),
            FrameError::Io,
        ),
    ];
    for (input, expected) in cases {
        let budget = PayloadBudget::new();
        let (mut queue, pump) = stdout_queue(input, end(), budget.clone());
        pump.await;
        let independent = budget
            .try_admit(bytes(MAX_BYTES - b"PRIVATE_PAYLOAD".len()))
            .unwrap();
        assert_eq!(
            budget.try_admit(bytes(/*len*/ 1)).err(),
            Some(AdmissionError::ByteOverflow)
        );
        let error = queue.receive(end()).await.err();
        assert_eq!(error, Some(StdoutError::Frame(expected)));
        assert!(!format!("{error:?}").contains("PRIVATE_"));
        let reclaimed = budget.try_admit(bytes(b"PRIVATE_PAYLOAD".len())).unwrap();
        assert_eq!(
            budget.try_admit(bytes(/*len*/ 1)).err(),
            Some(AdmissionError::ByteOverflow)
        );
        drop((independent, reclaimed));
    }
}

#[tokio::test(start_paused = true)]
async fn expired_receive_reclaims_queued_credits_but_not_independent_ownership() {
    let budget = PayloadBudget::new();
    let (mut queue, pump) = stdout_queue(b"queued\n".as_slice(), end(), budget.clone());
    pump.await;
    let independent = budget
        .try_admit(bytes(MAX_BYTES - b"queued".len()))
        .unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    assert_eq!(
        queue.receive(Instant::now()).await.err(),
        Some(StdoutError::Deadline)
    );
    let reclaimed = budget.try_admit(bytes(b"queued".len())).unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop((independent, reclaimed));
}

#[tokio::test]
async fn observing_dropped_pump_reclaims_queued_credits() {
    let budget = PayloadBudget::new();
    let (mut queue, pump) = stdout_queue(b"queued\npartial".as_slice(), end(), budget.clone());
    let mut pump = Box::pin(pump);
    pending(pump.as_mut()).await;
    // The reader yields after the delimiter; the next poll admits and queues it.
    pending(pump.as_mut()).await;
    let independent = budget
        .try_admit(bytes(MAX_BYTES - b"queued".len()))
        .unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop(pump);
    assert_eq!(
        queue.receive(end()).await.err(),
        Some(StdoutError::PumpStopped)
    );
    let reclaimed = budget.try_admit(bytes(b"queued".len())).unwrap();
    assert_eq!(
        budget.try_admit(bytes(/*len*/ 1)).err(),
        Some(AdmissionError::ByteOverflow)
    );
    drop((independent, reclaimed));
}
