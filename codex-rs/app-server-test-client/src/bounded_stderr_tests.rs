use super::*;
use pretty_assertions::assert_eq;
use std::collections::VecDeque;
use std::future::Future;
use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadBuf;
use tokio::io::duplex;

enum Step {
    Data(Vec<u8>),
    Wait,
    Eof,
    Io,
    Always,
}

#[derive(Default)]
struct Observed {
    steps: VecDeque<Step>,
    polls: usize,
    reads: usize,
    pending: usize,
    drops: usize,
    waker: Option<Waker>,
}
type Shared = Arc<Mutex<Observed>>;
struct Scripted(Shared);

impl AsyncRead for Scripted {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut state = self.0.lock().unwrap();
        state.polls += 1;
        match state.steps.pop_front().unwrap_or(Step::Eof) {
            Step::Data(bytes) => {
                let count = bytes.len().min(buf.remaining());
                buf.put_slice(&bytes[..count]);
                if count < bytes.len() {
                    state.steps.push_front(Step::Data(bytes[count..].to_vec()));
                }
                state.reads += 1;
                Poll::Ready(Ok(()))
            }
            Step::Wait => {
                state.steps.push_front(Step::Wait);
                state.pending += 1;
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Step::Eof => Poll::Ready(Ok(())),
            Step::Io => Poll::Ready(Err(io::Error::other("PRIVATE_IO_SENTINEL"))),
            Step::Always => {
                buf.put_slice(&vec![b'x'; buf.remaining()]);
                state.steps.push_front(Step::Always);
                state.reads += 1;
                Poll::Ready(Ok(()))
            }
        }
    }
}
impl Drop for Scripted {
    fn drop(&mut self) {
        self.0.lock().unwrap().drops += 1;
    }
}

fn reader(steps: Vec<Step>) -> (Scripted, Shared) {
    let steps = steps
        .into_iter()
        .filter(|step| !matches!(step, Step::Data(bytes) if bytes.is_empty()))
        .collect();
    let state = Arc::new(Mutex::new(Observed {
        steps,
        ..Observed::default()
    }));
    (Scripted(Arc::clone(&state)), state)
}
fn end() -> Instant {
    Instant::now() + Duration::from_secs(/*secs*/ 30)
}
fn pattern(length: usize) -> Vec<u8> {
    (0..length).map(|n| (n % 251) as u8).collect()
}
fn snapshot<R>(drain: &StderrDrain<R>) -> (Vec<u8>, u64, Option<StderrStop>) {
    let start = (drain.next + MAX_TAIL - drain.len) % MAX_TAIL;
    let bytes = drain
        .tail
        .iter()
        .cycle()
        .skip(start)
        .take(drain.len)
        .copied()
        .collect();
    (bytes, drain.discarded, drain.stopped)
}
async fn pending(mut future: Pin<&mut impl Future>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}
async fn cancel_at_reader_wait(drain: &mut StderrDrain<Scripted>, state: &Shared) {
    let mut future = Box::pin(drain.drain());
    while state.lock().unwrap().pending == 0 {
        pending(future.as_mut()).await;
    }
    drop(future);
}
fn release(state: &Shared, steps: Vec<Step>) {
    let waker = {
        let mut state = state.lock().unwrap();
        assert!(matches!(state.steps.pop_front(), Some(Step::Wait)));
        state.steps.extend(steps);
        state.pending = 0;
        state.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}
async fn assert_sticky(drain: &mut StderrDrain<Scripted>, state: &Shared) {
    let stop = drain.stopped.unwrap();
    let before = (snapshot(drain), state.lock().unwrap().polls);
    let advance =
        drain.end.saturating_duration_since(Instant::now()) + Duration::from_secs(/*secs*/ 1);
    tokio::time::advance(advance).await;
    assert_eq!(drain.drain().await, stop);
    assert_eq!((snapshot(drain), state.lock().unwrap().polls), before);
}

#[tokio::test(start_paused = true)]
async fn producer_completes_beyond_retention_while_open_then_exact_tail_reaches_eof() {
    let input = pattern(MAX_TAIL * 4 + 137);
    let (mut producer, stream) = duplex(/*max_buf_size*/ 1024);
    let mut writing = Box::pin(producer.write_all(&input));
    pending(writing.as_mut()).await;
    let mut drain = StderrDrain::new(stream, end());
    let mut draining = Box::pin(drain.drain());
    tokio::select! {
        result = &mut writing => result.unwrap(),
        stop = &mut draining => panic!("drain stopped before open producer completed: {stop:?}"),
    }
    drop((draining, writing));
    assert_eq!((drain.len, drain.stopped), (MAX_TAIL, None));
    assert!(drain.discarded as usize >= input.len() - 1024 - MAX_TAIL);
    drop(producer);
    assert_eq!(drain.drain().await, StderrStop::Eof);
    assert_eq!(
        snapshot(&drain),
        (
            input[input.len() - MAX_TAIL..].to_vec(),
            (input.len() - MAX_TAIL) as u64,
            Some(StderrStop::Eof),
        )
    );
}

#[tokio::test(start_paused = true)]
async fn cancellation_after_read_or_confirmed_pending_read_preserves_raw_bytes_and_owner() {
    for boundary in ["yield", "reader"] {
        let prefix = b"A\xe2";
        let rest = b"\x82\xac\0\r\n\xf0\x9f";
        let (stream, state) = reader(vec![Step::Data(prefix.to_vec()), Step::Wait]);
        let original_end = end();
        let mut drain = StderrDrain::new(stream, original_end);
        if boundary == "yield" {
            let mut future = Box::pin(drain.drain());
            pending(future.as_mut()).await;
            assert_eq!(state.lock().unwrap().reads, 1);
            assert_eq!(state.lock().unwrap().pending, 0);
            drop(future);
        } else {
            cancel_at_reader_wait(&mut drain, &state).await;
        }
        assert_eq!(
            (snapshot(&drain), drain.end, state.lock().unwrap().drops),
            ((prefix.to_vec(), 0, None), original_end, 0)
        );
        release(&state, vec![Step::Data(rest.to_vec()), Step::Eof]);
        assert_eq!(drain.drain().await, StderrStop::Eof);
        assert_eq!(
            snapshot(&drain),
            (
                [prefix.as_slice(), rest.as_slice()].concat(),
                0,
                Some(StderrStop::Eof)
            )
        );
        assert_sticky(&mut drain, &state).await;
        assert_eq!(state.lock().unwrap().drops, 0);
        drop(drain);
        assert_eq!(state.lock().unwrap().drops, 1);
    }
}

#[tokio::test(start_paused = true)]
async fn gated_wraps_preserve_the_logical_suffix_and_exact_eviction_count() {
    let mut all = pattern(MAX_TAIL - 3);
    let (stream, state) = reader(vec![Step::Data(all.clone()), Step::Wait]);
    let mut drain = StderrDrain::new(stream, end());
    cancel_at_reader_wait(&mut drain, &state).await;
    assert_eq!(
        (snapshot(&drain), drain.next),
        ((all.clone(), 0, None), all.len())
    );
    all.extend_from_slice(b"abcdef");
    release(&state, vec![Step::Data(b"abcdef".to_vec()), Step::Wait]);
    cancel_at_reader_wait(&mut drain, &state).await;
    assert_eq!(
        (snapshot(&drain), drain.next),
        ((all[3..].to_vec(), 3, None), 3)
    );
    let more = pattern(MAX_TAIL + 17);
    all.extend_from_slice(&more);
    release(&state, vec![Step::Data(more), Step::Eof]);
    assert_eq!(drain.drain().await, StderrStop::Eof);
    assert_eq!(
        snapshot(&drain),
        (
            all[all.len() - MAX_TAIL..].to_vec(),
            (all.len() - MAX_TAIL) as u64,
            Some(StderrStop::Eof),
        )
    );
    assert_sticky(&mut drain, &state).await;
}

#[tokio::test(start_paused = true)]
async fn dynamically_filled_ring_keeps_draining_after_discard_counter_saturates() {
    let mut all = pattern(MAX_TAIL);
    let (stream, state) = reader(vec![Step::Data(all.clone()), Step::Wait]);
    let mut drain = StderrDrain::new(stream, end());
    cancel_at_reader_wait(&mut drain, &state).await;
    assert_eq!(snapshot(&drain), (all.clone(), 0, None));
    drain.discarded = u64::MAX - 2;
    for bytes in [b"abcde", b"fghij"] {
        all.extend_from_slice(bytes);
        release(&state, vec![Step::Data(bytes.to_vec()), Step::Wait]);
        cancel_at_reader_wait(&mut drain, &state).await;
        assert_eq!(
            snapshot(&drain),
            (all[all.len() - MAX_TAIL..].to_vec(), u64::MAX, None)
        );
    }
    release(&state, vec![Step::Eof]);
    assert_eq!(drain.drain().await, StderrStop::Eof);
    assert_sticky(&mut drain, &state).await;
}

#[tokio::test(start_paused = true)]
async fn empty_and_unterminated_eof_and_secret_io_failure_are_sticky_without_more_polls() {
    for (input, last, expected) in [
        (b"".as_slice(), Step::Eof, StderrStop::Eof),
        (b"unterminated".as_slice(), Step::Eof, StderrStop::Eof),
        (b"PRIVATE_PAYLOAD".as_slice(), Step::Io, StderrStop::Io),
    ] {
        let (stream, state) = reader(vec![Step::Data(input.to_vec()), last]);
        let mut drain = StderrDrain::new(stream, end());
        assert_eq!(drain.drain().await, expected);
        assert_eq!(snapshot(&drain), (input.to_vec(), 0, Some(expected)));
        assert!(!format!("{expected:?}").contains("PRIVATE_"));
        assert_sticky(&mut drain, &state).await;
        assert_eq!(state.lock().unwrap().drops, 0);
        drop(drain);
        assert_eq!(state.lock().unwrap().drops, 1);
    }
}

#[tokio::test(start_paused = true)]
async fn already_expired_admission_never_polls_ready_input_or_drops_the_reader() {
    {
        let (stream, state) = reader(vec![Step::Wait]);
        drop(StderrDrain::new(stream, end()));
        let state = state.lock().unwrap();
        assert_eq!((state.polls, state.drops), (0, 1));
    }
    let (stream, state) = reader(vec![Step::Data(b"ready".to_vec())]);
    let mut drain = StderrDrain::new(stream, Instant::now());
    assert_eq!(drain.drain().await, StderrStop::Deadline);
    assert_eq!(
        (snapshot(&drain), state.lock().unwrap().polls),
        ((Vec::new(), 0, Some(StderrStop::Deadline)), 0)
    );
    assert_sticky(&mut drain, &state).await;
    assert_eq!(state.lock().unwrap().drops, 0);
    drop(drain);
    assert_eq!(state.lock().unwrap().drops, 1);
}

#[tokio::test(start_paused = true)]
async fn continuously_ready_reads_yield_bounded_work_without_renewing_the_end() {
    let (stream, state) = reader(vec![Step::Always]);
    let deadline = end();
    let mut drain = StderrDrain::new(stream, deadline);
    let mut future = Box::pin(drain.drain());
    for _ in 0..3 {
        let before = state.lock().unwrap().reads;
        pending(future.as_mut()).await;
        assert!(state.lock().unwrap().reads - before <= 1);
    }
    let reads = state.lock().unwrap().reads;
    assert!(reads > 0);
    tokio::time::advance(deadline - Instant::now()).await;
    assert_eq!(future.await, StderrStop::Deadline);
    assert_eq!(
        snapshot(&drain),
        (vec![b'x'; reads * CHUNK], 0, Some(StderrStop::Deadline))
    );
    assert_sticky(&mut drain, &state).await;
}

#[tokio::test(start_paused = true)]
async fn pending_success_eof_or_error_at_expiry_commits_bytes_before_deadline_classification() {
    for (observation, expected) in [
        (Step::Data(b"at-boundary".to_vec()), b"at-boundary".to_vec()),
        (Step::Eof, Vec::new()),
        (Step::Io, Vec::new()),
    ] {
        let (stream, state) = reader(vec![Step::Wait]);
        let deadline = end();
        let mut drain = StderrDrain::new(stream, deadline);
        let mut future = Box::pin(drain.drain());
        pending(future.as_mut()).await;
        assert!(state.lock().unwrap().pending > 0);
        tokio::time::advance(deadline - Instant::now()).await;
        let polls = state.lock().unwrap().polls;
        release(&state, vec![observation]);
        assert_eq!(future.await, StderrStop::Deadline);
        assert_eq!(state.lock().unwrap().polls, polls + 1);
        assert_eq!(snapshot(&drain), (expected, 0, Some(StderrStop::Deadline)));
        assert_sticky(&mut drain, &state).await;
    }
}

#[tokio::test(start_paused = true)]
async fn blocked_silent_or_partial_read_times_out_without_dropping_owned_reader() {
    for prefix in [b"".as_slice(), b"prefix".as_slice()] {
        let (stream, state) = reader(vec![Step::Data(prefix.to_vec()), Step::Wait]);
        let deadline = end();
        let mut drain = StderrDrain::new(stream, deadline);
        drop(Box::pin(drain.drain()));
        assert_eq!((state.lock().unwrap().polls, drain.stopped), (0, None));
        cancel_at_reader_wait(&mut drain, &state).await;
        let mut future = Box::pin(drain.drain());
        pending(future.as_mut()).await;
        tokio::time::advance(deadline - Instant::now()).await;
        assert_eq!(future.await, StderrStop::Deadline);
        assert_eq!(
            (snapshot(&drain), state.lock().unwrap().drops),
            ((prefix.to_vec(), 0, Some(StderrStop::Deadline)), 0)
        );
        assert_sticky(&mut drain, &state).await;
        drop(drain);
        assert_eq!(state.lock().unwrap().drops, 1);
    }
}
