use std::io;
use std::io::Read;

use pretty_assertions::assert_eq;

use crate::FailureStage;
use crate::Response;
use crate::ResponseError;
use crate::ResponseEvent;
use crate::decode_response;
use crate::encode_response;
use crate::read_response_frame;

const NONCE: [u8; 16] = [7; 16];

fn responses() -> Vec<Response> {
    [
        ResponseEvent::Ready { worker_pid: 7 },
        ResponseEvent::Spawned { target_pid: 9 },
        ResponseEvent::RootExited { exit_code: u32::MAX },
        ResponseEvent::Failed { stage: FailureStage::Control, win32_code: 5 },
    ]
    .into_iter()
    .map(|event| Response { nonce: NONCE, event })
    .collect()
}

#[test]
fn consecutive_frames_leave_the_exact_next_frame_untouched() {
    let responses = responses();
    let frames: Vec<_> = responses.iter().map(|r| encode_response(r, &NONCE).unwrap()).collect();
    let joined = frames.concat();
    let mut reader = joined.as_slice();
    for (index, response) in responses.into_iter().enumerate() {
        assert_eq!(read_response_frame(&mut reader, &NONCE), Ok(response));
        assert_eq!(reader, frames[index + 1..].concat());
    }
    // Even after root/failure observations, a required boundary is not optional.
    assert_eq!(read_response_frame(&mut reader, &NONCE), Err(ResponseError::Framing));
}

struct InterruptedBytes<'a> {
    bytes: &'a [u8],
    interrupt_next: bool,
}

impl Read for InterruptedBytes<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.interrupt_next = !self.interrupt_next;
        if self.interrupt_next {
            return Err(io::ErrorKind::Interrupted.into());
        }
        assert!(!self.bytes.is_empty(), "reader probed beyond the frame");
        let count = buffer.len().min(1);
        buffer[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        Ok(count)
    }
}

#[test]
fn interrupted_one_byte_reads_never_probe_after_the_frame() {
    for response in responses() {
        let frame = encode_response(&response, &NONCE).unwrap();
        let mut reader = InterruptedBytes { bytes: &frame, interrupt_next: false };
        assert_eq!(read_response_frame(&mut reader, &NONCE), Ok(response));
        assert_eq!(reader.bytes, b"");
    }
}

struct BodyProbe<'a> {
    header: &'a [u8],
    requested: Vec<usize>,
}

impl Read for BodyProbe<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.header.is_empty() {
            return self.header.read(buffer);
        }
        self.requested.push(buffer.len());
        Err(io::Error::other("private body detail"))
    }
}

#[test]
fn length_is_admitted_before_body_access_and_bounds_allocation() {
    for length in [0_u32, 1_048_577, u32::MAX] {
        let header = length.to_le_bytes();
        let mut reader = BodyProbe { header: &header, requested: vec![] };
        assert_eq!(read_response_frame(&mut reader, &NONCE), Err(ResponseError::Framing));
        assert_eq!(reader.requested, Vec::<usize>::new());
    }
    let header = 1_048_576_u32.to_le_bytes();
    let mut reader = BodyProbe { header: &header, requested: vec![] };
    assert_eq!(read_response_frame(&mut reader, &NONCE), Err(ResponseError::Framing));
    assert_eq!(reader.requested, vec![1_048_576]);
}

struct FailAfter<'a>(&'a [u8]);

impl Read for FailAfter<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() {
            return Err(io::Error::other("private transport detail"));
        }
        self.0.read(buffer)
    }
}

#[test]
fn every_truncation_and_io_failure_refuses_with_static_framing_error() {
    for response in responses() {
        let frame = encode_response(&response, &NONCE).unwrap();
        for end in 0..frame.len() {
            assert_eq!(read_response_frame(&mut &frame[..end], &NONCE), Err(ResponseError::Framing));
            let error = read_response_frame(&mut FailAfter(&frame[..end]), &NONCE).unwrap_err();
            assert_eq!(error.to_string(), "invalid console response framing");
            assert_eq!(error, ResponseError::Framing);
        }
    }
}

#[test]
fn payload_and_nonce_errors_leave_following_frame_untouched() {
    let good = encode_response(&responses().remove(0), &NONCE).unwrap();
    let payload = std::str::from_utf8(&good[4..]).unwrap();
    for bad_payload in [
        "!".to_owned(),
        payload.replacen("\"version\":1", "\"version\":2", 1),
        payload.replacen("\"workerPid\":7", "\"workerPid\":0", 1),
        payload.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
    ] {
        let mut frame = (bad_payload.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(bad_payload.as_bytes());
        let expected = decode_response(&frame, &NONCE);
        assert!(expected.is_err());
        frame.extend_from_slice(&good);
        let mut reader = frame.as_slice();
        assert_eq!(read_response_frame(&mut reader, &NONCE), expected);
        assert_eq!(reader, good);
    }
    let joined = [good.as_slice(), good.as_slice()].concat();
    let mut reader = joined.as_slice();
    assert_eq!(read_response_frame(&mut reader, &[8; 16]), Err(ResponseError::Admission));
    assert_eq!(reader, good);
}
