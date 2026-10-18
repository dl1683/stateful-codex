use super::*;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::DuplexStream;
use tokio::io::duplex;

fn end() -> Instant {
    Instant::now() + Duration::from_secs(/*secs*/ 30)
}

fn frame(budget: &PayloadBudget, bytes: &[u8]) -> LeasedFrame {
    budget.try_admit(bytes.into()).unwrap()
}

fn hold_rest(budget: &PayloadBudget, used: usize) -> LeasedFrame {
    budget
        .try_admit(vec![b'x'; MAX_BYTES - used].into_boxed_slice())
        .unwrap()
}

fn exhausted(budget: &PayloadBudget) {
    assert_eq!(
        budget.try_admit(b"x".as_slice().into()).err(),
        Some(AdmissionError::ByteOverflow)
    );
}

fn recovered(budget: &PayloadBudget, len: usize) {
    assert!(budget.try_admit(vec![b'x'; len].into_boxed_slice()).is_ok());
}

async fn pending(mut future: Pin<&mut impl Future>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

async fn no_more(peer: &mut DuplexStream) {
    let mut byte = [0];
    pending(Box::pin(peer.read_exact(&mut byte)).as_mut()).await;
}

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    PendingFlush,
    IoWrite,
    IoFlush,
}

#[derive(Default)]
struct Capture {
    bytes: Mutex<Vec<u8>>,
    flushes: AtomicUsize,
}

struct Output {
    mode: Mode,
    capture: Arc<Capture>,
}

fn output(mode: Mode) -> (Output, Arc<Capture>) {
    let capture = Arc::new(Capture::default());
    (
        Output {
            mode,
            capture: Arc::clone(&capture),
        },
        capture,
    )
}

impl AsyncWrite for Output {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.mode {
            Mode::IoWrite => Poll::Ready(Err(io::Error::other("PRIVATE_IO_SENTINEL"))),
            Mode::Normal | Mode::PendingFlush | Mode::IoFlush => {
                self.capture.bytes.lock().unwrap().extend_from_slice(bytes);
                Poll::Ready(Ok(bytes.len()))
            }
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.capture.flushes.fetch_add(/*val*/ 1, Ordering::SeqCst);
        match self.mode {
            Mode::PendingFlush => Poll::Pending,
            Mode::IoFlush => Poll::Ready(Err(io::Error::other("PRIVATE_IO_SENTINEL"))),
            Mode::Normal | Mode::IoWrite => Poll::Ready(Ok(())),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn completion_releases_bytes_and_empty_message_credit_before_future_destruction() {
    let budget = PayloadBudget::new();
    let (output, capture) = output(Mode::Normal);
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 3);
    let mut send = Box::pin(writer.send(frame(&budget, b"abc"), end()));
    exhausted(&budget);
    assert_eq!(send.as_mut().await, Ok(()));
    recovered(&budget, /*len*/ 3);
    drop(send);
    drop(rest);
    let held: Vec<_> = (0..MAX_MESSAGES - 1).map(|_| frame(&budget, b"")).collect();
    let mut send = Box::pin(writer.send(frame(&budget, b""), end()));
    assert_eq!(
        budget.try_admit(Box::default()).err(),
        Some(AdmissionError::MessageOverflow)
    );
    assert_eq!(send.as_mut().await, Ok(()));
    assert!(budget.try_admit(Box::default()).is_ok());
    assert_eq!(*capture.bytes.lock().unwrap(), b"abc\n\n");
    drop((send, held));
}

#[tokio::test]
async fn unpolled_owning_future_retains_credit_then_drops_without_poisoning() {
    let budget = PayloadBudget::new();
    let (output, capture) = output(Mode::Normal);
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 3);
    let send = Box::pin(writer.send(frame(&budget, b"abc"), end()));
    exhausted(&budget);
    assert!(capture.bytes.lock().unwrap().is_empty());
    drop(send);
    recovered(&budget, /*len*/ 3);
    assert_eq!(writer.send(frame(&budget, b"ok"), end()).await, Ok(()));
    assert_eq!(*capture.bytes.lock().unwrap(), b"ok\n");
    drop(rest);
}

#[tokio::test]
async fn confirmed_prebyte_blockage_cancellation_releases_credit_and_poisons() {
    let budget = PayloadBudget::new();
    let (mut output, mut peer) = duplex(/*max_buf_size*/ 1);
    output.write_all(b"!").await.unwrap();
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 1);
    let mut send = Box::pin(writer.send(frame(&budget, b"x"), end()));
    pending(send.as_mut()).await;
    exhausted(&budget);
    let mut prefix = [0];
    peer.read_exact(&mut prefix).await.unwrap();
    assert_eq!(prefix, *b"!");
    drop(send);
    recovered(&budget, /*len*/ 1);
    assert_eq!(
        writer.send(frame(&budget, b"z"), end()).await,
        Err(SendError::Frame(FrameError::Poisoned))
    );
    no_more(&mut peer).await;
    drop(rest);
}

#[tokio::test]
async fn actual_partial_output_cancellation_releases_the_whole_lease_without_replay() {
    let budget = PayloadBudget::new();
    let (output, mut peer) = duplex(/*max_buf_size*/ 1);
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 3);
    let mut send = Box::pin(writer.send(frame(&budget, b"abc"), end()));
    pending(send.as_mut()).await;
    let mut prefix = [0];
    peer.read_exact(&mut prefix).await.unwrap();
    assert_eq!(prefix, *b"a");
    exhausted(&budget);
    drop(send);
    recovered(&budget, /*len*/ 3);
    assert_eq!(
        writer.send(frame(&budget, b"new"), end()).await,
        Err(SendError::Frame(FrameError::Poisoned))
    );
    no_more(&mut peer).await;
    drop(rest);
}

#[tokio::test(start_paused = true)]
async fn active_partial_write_expires_at_its_original_end_and_releases_before_drop() {
    let budget = PayloadBudget::new();
    let (output, mut peer) = duplex(/*max_buf_size*/ 1);
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 3);
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 1);
    let mut send = Box::pin(writer.send(frame(&budget, b"abc"), deadline));
    pending(send.as_mut()).await;
    let mut prefix = [0];
    peer.read_exact(&mut prefix).await.unwrap();
    assert_eq!(prefix, *b"a");
    exhausted(&budget);
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    assert_eq!(
        send.as_mut().await,
        Err(SendError::Frame(FrameError::Deadline))
    );
    recovered(&budget, /*len*/ 3);
    drop(send);
    assert_eq!(
        writer.send(frame(&budget, b"new"), end()).await,
        Err(SendError::Frame(FrameError::Poisoned))
    );
    drop(rest);
}

#[tokio::test]
async fn pending_flush_keeps_all_written_payload_charged_until_owning_future_drops() {
    let budget = PayloadBudget::new();
    let (output, capture) = output(Mode::PendingFlush);
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 3);
    let mut send = Box::pin(writer.send(frame(&budget, b"abc"), end()));
    while capture.flushes.load(Ordering::SeqCst) == 0 {
        pending(send.as_mut()).await;
    }
    assert_eq!(*capture.bytes.lock().unwrap(), b"abc\n");
    exhausted(&budget);
    drop(send);
    recovered(&budget, /*len*/ 3);
    assert_eq!(
        writer.send(frame(&budget, b"new"), end()).await,
        Err(SendError::Frame(FrameError::Poisoned))
    );
    assert_eq!(*capture.bytes.lock().unwrap(), b"abc\n");
    drop(rest);
}

#[tokio::test(start_paused = true)]
async fn already_expired_preflight_releases_without_writing_or_poisoning() {
    let budget = PayloadBudget::new();
    let (output, capture) = output(Mode::Normal);
    let mut writer = LeasedWriter::new(output, budget.clone());
    let rest = hold_rest(&budget, /*used*/ 1);
    let mut send = Box::pin(writer.send(frame(&budget, b"x"), Instant::now()));
    assert_eq!(
        send.as_mut().await,
        Err(SendError::Frame(FrameError::Deadline))
    );
    recovered(&budget, /*len*/ 1);
    assert!(capture.bytes.lock().unwrap().is_empty());
    drop((send, rest));
    assert_eq!(writer.send(frame(&budget, b"ok"), end()).await, Ok(()));
    assert_eq!(*capture.bytes.lock().unwrap(), b"ok\n");
}

#[tokio::test]
async fn write_and_flush_io_errors_release_normalize_and_permanently_poison() {
    for mode in [Mode::IoWrite, Mode::IoFlush] {
        let budget = PayloadBudget::new();
        let (output, _) = output(mode);
        let mut writer = LeasedWriter::new(output, budget.clone());
        let payload = b"PRIVATE_FRAME";
        let rest = hold_rest(&budget, payload.len());
        let mut send = Box::pin(writer.send(frame(&budget, payload), end()));
        let result = send.as_mut().await;
        assert_eq!(result, Err(SendError::Frame(FrameError::Io)));
        assert!(!format!("{result:?}").contains("PRIVATE_"));
        recovered(&budget, payload.len());
        drop(send);
        assert_eq!(
            writer.send(frame(&budget, b"x"), end()).await,
            Err(SendError::Frame(FrameError::Poisoned))
        );
        drop(rest);
    }
}

#[tokio::test]
async fn invalid_frames_release_without_output_poisoning_or_payload_diagnostics() {
    for (raw, expected) in [
        (
            b"PRIVATE_PAYLOAD\n".to_vec().into_boxed_slice(),
            FrameError::EmbeddedNewline,
        ),
        (
            vec![b'x'; MAX_BYTES / 2 + 1].into_boxed_slice(),
            FrameError::TooLarge,
        ),
    ] {
        let budget = PayloadBudget::new();
        let (output, capture) = output(Mode::Normal);
        let mut writer = LeasedWriter::new(output, budget.clone());
        let len = raw.len();
        let rest = hold_rest(&budget, len);
        let result = writer.send(budget.try_admit(raw).unwrap(), end()).await;
        assert_eq!(result, Err(SendError::Frame(expected)));
        assert!(!format!("{result:?}").contains("PRIVATE_"));
        recovered(&budget, len);
        assert!(capture.bytes.lock().unwrap().is_empty());
        drop(rest);
        assert_eq!(writer.send(frame(&budget, b"ok"), end()).await, Ok(()));
        assert_eq!(*capture.bytes.lock().unwrap(), b"ok\n");
    }
}

#[tokio::test]
async fn foreign_pool_refusal_releases_origin_and_leaves_local_writer_clean() {
    let origin = PayloadBudget::new();
    let local = PayloadBudget::new();
    let (output, capture) = output(Mode::Normal);
    let mut writer = LeasedWriter::new(output, local.clone());
    let payload = b"PRIVATE_FRAME";
    let rest = hold_rest(&origin, payload.len());
    let mut send = Box::pin(writer.send(frame(&origin, payload), end()));
    exhausted(&origin);
    let result = send.as_mut().await;
    assert_eq!(result, Err(SendError::ForeignBudget));
    assert!(!format!("{result:?}").contains("PRIVATE_"));
    recovered(&origin, payload.len());
    recovered(&local, MAX_BYTES);
    assert!(capture.bytes.lock().unwrap().is_empty());
    drop(send);
    assert_eq!(writer.send(frame(&local, b"ok"), end()).await, Ok(()));
    assert_eq!(*capture.bytes.lock().unwrap(), b"ok\n");
    drop(rest);
}
