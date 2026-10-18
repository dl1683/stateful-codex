use super::*;
use pretty_assertions::assert_eq;
use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::ReadBuf;
use tokio::io::duplex;

fn end() -> Instant {
    Instant::now() + Duration::from_secs(/*secs*/ 30)
}

fn payload(
    result: Result<Option<LeasedFrame>, StdoutError>,
) -> Result<Option<Vec<u8>>, StdoutError> {
    result.map(|frame| frame.map(|frame| frame.bytes().to_vec()))
}

async fn receive(queue: &mut StdoutQueue) -> Result<Option<Vec<u8>>, StdoutError> {
    payload(queue.receive(end()).await)
}

async fn pending(mut future: Pin<&mut impl Future>) {
    poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test]
async fn ordered_frames_and_clean_eof_remain_sticky() {
    for input in ["", "a\n\n€\n"] {
        let (mut queue, pump) = stdout_queue(input.as_bytes(), end());
        pump.await;
        let mut actual = Vec::new();
        while let Some(frame) = queue.receive(end()).await.unwrap() {
            actual.push(frame.bytes().to_vec());
        }
        let expected: Vec<_> = input
            .split_terminator('\n')
            .map(|s| s.as_bytes().to_vec())
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(payload(queue.receive(Instant::now()).await), Ok(None));
    }
}

#[tokio::test]
async fn retained_byte_leases_allow_empty_frames_and_refuse_the_first_extra_byte() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 8192);
    let (mut queue, pump) = stdout_queue(input, end());
    let consumer = async {
        let mut held = Vec::new();
        for _ in 0..2 {
            producer
                .write_all(&vec![b'x'; MAX_BYTES / 2])
                .await
                .unwrap();
            producer.write_all(b"\n").await.unwrap();
            let frame = queue.receive(end()).await.unwrap().unwrap();
            assert_eq!(frame.bytes(), vec![b'x'; MAX_BYTES / 2]);
            held.push(frame);
        }
        producer.write_all(b"\n").await.unwrap();
        let empty = queue.receive(end()).await.unwrap().unwrap();
        assert_eq!(empty.bytes(), b"");
        producer.write_all(b"z\n").await.unwrap();
        assert_eq!(receive(&mut queue).await, Err(StdoutError::ByteOverflow));
        assert_eq!(queue.frames.len(), 0);
        drop((held, empty));
    };
    tokio::join!(pump, consumer);
}

#[tokio::test]
async fn dropping_a_delivered_frame_returns_byte_and_message_credit() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 8192);
    let (mut queue, pump) = stdout_queue(input, end());
    tokio::join!(pump, async {
        let mut held = Vec::new();
        for _ in 0..3 {
            if held.len() == 2 {
                held.remove(/*index*/ 0);
            }
            producer
                .write_all(&vec![b'x'; MAX_BYTES / 2])
                .await
                .unwrap();
            producer.write_all(b"\n").await.unwrap();
            let frame = queue.receive(end()).await.unwrap().unwrap();
            assert_eq!(frame.bytes(), vec![b'x'; MAX_BYTES / 2]);
            held.push(frame);
        }
        drop(held);
        for _ in 0..MAX_MESSAGES + 1 {
            producer.write_all(b"\n").await.unwrap();
            assert_eq!(receive(&mut queue).await, Ok(Some(Vec::new())));
        }
        drop(producer);
        assert_eq!(receive(&mut queue).await, Ok(None));
    });
}

#[tokio::test]
async fn full_queue_reports_terminal_overflow_without_delivering_queued_success() {
    let input = io::Cursor::new(vec![b'\n'; MAX_MESSAGES + 1]);
    let (mut queue, pump) = stdout_queue(input, end());
    pump.await;
    assert_eq!(queue.frames.len(), MAX_MESSAGES);
    assert_eq!(receive(&mut queue).await, Err(StdoutError::MessageOverflow));
    assert_eq!(queue.frames.len(), 0);
    assert_eq!(receive(&mut queue).await, Err(StdoutError::MessageOverflow));
}

#[tokio::test]
async fn delivered_empty_frames_still_exhaust_message_credit() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 128);
    let (mut queue, pump) = stdout_queue(input, end());
    tokio::join!(pump, async {
        let mut held = Vec::new();
        for _ in 0..MAX_MESSAGES {
            producer.write_all(b"\n").await.unwrap();
            held.push(queue.receive(end()).await.unwrap().unwrap());
        }
        assert!(queue.frames.is_empty());
        producer.write_all(b"\n").await.unwrap();
        assert_eq!(receive(&mut queue).await, Err(StdoutError::MessageOverflow));
        drop(held);
    });
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
async fn reader_failures_win_over_queued_frames_and_expiry_without_payload_diagnostics() {
    let cases: Vec<(Box<dyn AsyncRead + Unpin + Send>, FrameError)> = vec![
        (
            Box::new(b"PRIVATE_PAYLOAD\npartial".as_slice()),
            FrameError::PartialEof,
        ),
        (
            Box::new(io::Cursor::new(vec![b'x'; 8 * 1024 * 1024 + 1])),
            FrameError::TooLarge,
        ),
        (
            Box::new(b"PRIVATE_PAYLOAD\n".as_slice().chain(Broken)),
            FrameError::Io,
        ),
    ];
    for (input, expected) in cases {
        let (mut queue, pump) = stdout_queue(input, end());
        pump.await;
        let result = payload(queue.receive(Instant::now()).await);
        assert_eq!(result, Err(StdoutError::Frame(expected)));
        assert!(!format!("{result:?}").contains("PRIVATE_"));
        assert_eq!(queue.frames.len(), 0);
    }
}

#[tokio::test]
async fn dropped_pump_before_poll_or_after_queuing_is_not_eof() {
    let (mut queue, pump) = stdout_queue(b"unused\n".as_slice(), end());
    drop(pump);
    assert_eq!(receive(&mut queue).await, Err(StdoutError::PumpStopped));
    let (mut queue, pump) = stdout_queue(b"queued\npartial".as_slice(), end());
    let mut pump = Box::pin(pump);
    while queue.frames.is_empty() {
        pending(pump.as_mut()).await;
    }
    drop(pump);
    for _ in 0..2 {
        assert_eq!(receive(&mut queue).await, Err(StdoutError::PumpStopped));
    }
    assert_eq!(queue.frames.len(), 0);
}

#[tokio::test]
async fn ready_delivery_never_waits_for_terminal_and_cancelled_receive_preserves_input() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 64);
    producer.write_all(b"first\npre").await.unwrap();
    let (mut queue, pump) = stdout_queue(input, end());
    let mut pump = Box::pin(pump);
    while queue.frames.is_empty() {
        pending(pump.as_mut()).await;
    }
    {
        let receive = queue.receive(end());
        tokio::pin!(receive);
        poll_fn(|cx| match receive.as_mut().poll(cx) {
            Poll::Ready(result) => {
                assert_eq!(payload(result), Ok(Some(b"first".to_vec())));
                Poll::Ready(())
            }
            Poll::Pending => panic!("ready frame waited for terminal outcome"),
        })
        .await;
    }
    {
        let receive = queue.receive(end());
        tokio::pin!(receive);
        pending(receive.as_mut()).await;
    }
    producer.write_all(b"fix\n").await.unwrap();
    drop(producer);
    tokio::join!(pump, async {
        assert_eq!(receive(&mut queue).await, Ok(Some(b"prefix".to_vec())));
        assert_eq!(receive(&mut queue).await, Ok(None));
    });
}

#[tokio::test(start_paused = true)]
async fn expired_receive_seals_ready_data_and_wakes_silent_or_partial_pumps() {
    for prefix in ["", "partial"] {
        let (mut producer, input) = duplex(/*max_buf_size*/ 64);
        producer.write_all(prefix.as_bytes()).await.unwrap();
        let (mut queue, pump) = stdout_queue(input, end());
        let mut pump = Box::pin(pump);
        pending(pump.as_mut()).await;
        pending(pump.as_mut()).await;
        let deadline = Instant::now() + Duration::from_secs(/*secs*/ 1);
        let (_, result) = tokio::join!(pump, queue.receive(deadline));
        assert_eq!(payload(result), Err(StdoutError::Deadline));
        assert!(producer.write_all(b"x").await.is_err());
        assert_eq!(receive(&mut queue).await, Err(StdoutError::Deadline));
    }
    let (mut queue, pump) = stdout_queue(b"ready\n".as_slice(), end());
    pump.await;
    assert_eq!(
        payload(queue.receive(Instant::now()).await),
        Err(StdoutError::Deadline)
    );
}

#[tokio::test(start_paused = true)]
async fn traffic_does_not_renew_pump_end_and_drop_wakes_a_blocked_reader() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 64);
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 1);
    let (mut queue, pump) = stdout_queue(input, deadline);
    let task = tokio::spawn(pump);
    producer.write_all(b"notification\npartial").await.unwrap();
    assert_eq!(
        receive(&mut queue).await,
        Ok(Some(b"notification".to_vec()))
    );
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    task.await.unwrap();
    assert_eq!(receive(&mut queue).await, Err(StdoutError::Deadline));
    let (peer, input) = duplex(/*max_buf_size*/ 1);
    let (queue, pump) = stdout_queue(input, end());
    let mut pump = Box::pin(pump);
    pending(pump.as_mut()).await;
    drop(queue);
    pump.await;
    drop(peer);
}

#[tokio::test]
async fn closed_message_channel_waits_for_outcome_without_spinning_or_false_eof() {
    let (sender, frames) = mpsc::channel(MAX_MESSAGES);
    let (done, ending) = oneshot::channel();
    let mut queue = StdoutQueue {
        frames,
        ending,
        outcome: None,
        messages_closed: false,
        finished: false,
        pump_end: end(),
    };
    drop(sender);
    {
        let receive = queue.receive(end());
        tokio::pin!(receive);
        pending(receive.as_mut()).await;
    }
    done.send(Ok(())).unwrap();
    assert_eq!(receive(&mut queue).await, Ok(None));
    assert_eq!(payload(queue.receive(Instant::now()).await), Ok(None));
}
