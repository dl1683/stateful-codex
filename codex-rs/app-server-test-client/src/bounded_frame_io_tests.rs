use super::*;
use pretty_assertions::assert_eq;
use std::future::Future;
use std::future::poll_fn;
use std::task::Poll;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::duplex;

fn end() -> Instant {
    Instant::now() + Duration::from_secs(/*secs*/ 30)
}

#[tokio::test]
async fn fragmented_unicode_and_coalesced_frames_survive_cancelled_receives() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 16);
    let mut reader = FrameReader::new(input);
    producer.write_all(&[b'a', 0xe2]).await.unwrap();
    {
        let receive = reader.receive(end());
        tokio::pin!(receive);
        poll_fn(|cx| {
            assert!(receive.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    producer
        .write_all(&[0x82, 0xac, b'\n', b'b', b'\n'])
        .await
        .unwrap();
    producer.shutdown().await.unwrap();
    assert_eq!(
        reader.receive(end()).await,
        Ok(Some("a€".as_bytes().into()))
    );
    assert_eq!(
        reader.receive(end()).await,
        Ok(Some(b"b".as_slice().into()))
    );
    assert_eq!(reader.receive(end()).await, Ok(None));
}

#[tokio::test]
async fn cancellation_after_delimiter_keeps_completed_frame() {
    let mut reader = FrameReader::new(b"first\nsecond\n".as_slice());
    {
        let receive = reader.receive(end());
        tokio::pin!(receive);
        poll_fn(|cx| {
            assert!(receive.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert_eq!(
        reader.receive(end()).await,
        Ok(Some(b"first".as_slice().into()))
    );
    assert_eq!(
        reader.receive(end()).await,
        Ok(Some(b"second".as_slice().into()))
    );
}

#[tokio::test]
async fn exact_limit_round_trips_but_first_excess_byte_without_lf_refuses() {
    let payload = vec![b'x'; MAX_FRAME];
    let (output, input) = duplex(/*max_buf_size*/ 17);
    let mut writer = FrameWriter::new(output);
    let mut reader = FrameReader::new(input);
    let (written, read) = tokio::join!(writer.send(&payload, end()), reader.receive(end()));
    assert_eq!(written, Ok(()));
    assert_eq!(read, Ok(Some(payload.into_boxed_slice())));
    let excess = vec![b'x'; MAX_FRAME + 1];
    let mut reader = FrameReader::new(excess.as_slice());
    assert_eq!(reader.receive(end()).await, Err(FrameError::TooLarge));
    assert_eq!(reader.receive(end()).await, Err(FrameError::Poisoned));
}

#[tokio::test]
async fn incomplete_eof_poisons_without_disclosing_payload() {
    let mut reader = FrameReader::new(b"PRIVATE_SENTINEL".as_slice());
    let result = reader.receive(end()).await;
    assert_eq!(result, Err(FrameError::PartialEof));
    assert_eq!(format!("{result:?}"), "Err(PartialEof)");
    assert_eq!(reader.receive(end()).await, Err(FrameError::Poisoned));
}

#[tokio::test(start_paused = true)]
async fn expired_deadlines_refuse_ready_input_and_notifications_do_not_renew() {
    let mut reader = FrameReader::new(b"notification\nanswer\n".as_slice());
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 1);
    assert_eq!(
        reader.receive(deadline).await,
        Ok(Some(b"notification".as_slice().into()))
    );
    tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
    assert_eq!(reader.receive(deadline).await, Err(FrameError::Deadline));
    assert_eq!(
        reader.receive(end()).await,
        Ok(Some(b"answer".as_slice().into()))
    );
}

#[tokio::test(start_paused = true)]
async fn timed_out_read_retains_partial_bytes() {
    let (mut producer, input) = duplex(/*max_buf_size*/ 16);
    producer.write_all(b"prefix").await.unwrap();
    let mut reader = FrameReader::new(input);
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 1);
    {
        let receive = reader.receive(deadline);
        tokio::pin!(receive);
        poll_fn(|cx| {
            assert!(receive.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
        assert_eq!(receive.await, Err(FrameError::Deadline));
    }
    producer.write_all(b"-suffix\n").await.unwrap();
    assert_eq!(
        reader.receive(end()).await,
        Ok(Some(b"prefix-suffix".as_slice().into()))
    );
}

#[tokio::test(start_paused = true)]
async fn blocked_partial_write_expires_and_cannot_be_replayed() {
    let (output, mut peer) = duplex(/*max_buf_size*/ 1);
    let mut writer = FrameWriter::new(output);
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 1);
    {
        let send = writer.send(b"abc", deadline);
        tokio::pin!(send);
        poll_fn(|cx| {
            assert!(send.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        let mut first = [0; 1];
        peer.read_exact(&mut first).await.unwrap();
        assert_eq!(first, [b'a']);
        tokio::time::advance(Duration::from_secs(/*secs*/ 1)).await;
        assert_eq!(send.await, Err(FrameError::Deadline));
    }
    assert_eq!(writer.send(b"abc", end()).await, Err(FrameError::Poisoned));
}

#[tokio::test]
async fn dropping_a_partially_written_future_poisons_the_retained_writer() {
    let (output, mut peer) = duplex(/*max_buf_size*/ 1);
    let mut writer = FrameWriter::new(output);
    {
        let send = writer.send(b"abc", end());
        tokio::pin!(send);
        poll_fn(|cx| {
            assert!(send.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    let mut first = [0; 1];
    peer.read_exact(&mut first).await.unwrap();
    assert_eq!(first, [b'a']);
    assert_eq!(writer.send(b"abc", end()).await, Err(FrameError::Poisoned));
}

#[tokio::test]
async fn invalid_outbound_frames_do_not_write_or_poison_a_clean_stream() {
    let (output, mut peer) = duplex(/*max_buf_size*/ 8);
    let mut writer = FrameWriter::new(output);
    assert_eq!(
        writer.send(&vec![b'x'; MAX_FRAME + 1], end()).await,
        Err(FrameError::TooLarge)
    );
    assert_eq!(
        writer.send(b"two\nframes", end()).await,
        Err(FrameError::EmbeddedNewline)
    );
    assert_eq!(
        writer.send(b"ok", Instant::now()).await,
        Err(FrameError::Deadline)
    );
    writer.send(b"ok", end()).await.unwrap();
    let mut received = [0; 3];
    peer.read_exact(&mut received).await.unwrap();
    assert_eq!(received, *b"ok\n");
}

#[tokio::test]
async fn broken_output_poisons_without_payload_or_io_diagnostics() {
    let (output, peer) = duplex(/*max_buf_size*/ 1);
    drop(peer);
    let mut writer = FrameWriter::new(output);
    let result = writer.send(b"PRIVATE_SENTINEL", end()).await;
    assert_eq!(format!("{result:?}"), "Err(Io)");
    assert_eq!(
        writer.send(b"retry", end()).await,
        Err(FrameError::Poisoned)
    );
}
