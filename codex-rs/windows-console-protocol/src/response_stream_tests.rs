use std::io;
use std::io::Read;

use pretty_assertions::assert_eq;

use super::read_closed_response;
use crate::FailureStage;
use crate::Response;
use crate::ResponseError;
use crate::ResponseEvent;
use crate::decode_response;
use crate::encode_response;

const NONCE: [u8; 16] = [7; 16];

fn responses() -> Vec<Response> {
    let mut events = vec![
        ResponseEvent::Ready { worker_pid: 1 },
        ResponseEvent::Ready {
            worker_pid: u32::MAX,
        },
        ResponseEvent::Spawned { target_pid: 1 },
        ResponseEvent::Spawned {
            target_pid: u32::MAX,
        },
        ResponseEvent::RootExited { exit_code: 0 },
        ResponseEvent::RootExited {
            exit_code: u32::MAX,
        },
    ];
    for stage in [
        FailureStage::Bootstrap,
        FailureStage::Console,
        FailureStage::HandleAdmission,
        FailureStage::Spawn,
        FailureStage::Control,
        FailureStage::Internal,
    ] {
        for win32_code in [0, u32::MAX] {
            events.push(ResponseEvent::Failed { stage, win32_code });
        }
    }
    events
        .into_iter()
        .map(|event| Response {
            nonce: NONCE,
            event,
        })
        .collect()
}

struct InterruptedChunks<'a> {
    bytes: &'a [u8],
    chunk: usize,
    interrupt_next: bool,
}

impl Read for InterruptedChunks<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.interrupt_next = !self.interrupt_next;
        if self.interrupt_next {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let count = buffer.len().min(self.chunk).min(self.bytes.len());
        buffer[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        Ok(count)
    }
}

#[test]
fn partial_interrupted_reads_preserve_every_event_and_u32_value() {
    for response in responses() {
        let frame = encode_response(&response, &NONCE).unwrap();
        for chunk in [1, 2, 3, 4, 7, frame.len()] {
            let mut reader = InterruptedChunks {
                bytes: &frame,
                chunk,
                interrupt_next: false,
            };
            assert_eq!(
                read_closed_response(&mut reader, &NONCE),
                Ok(response.clone())
            );
            assert_eq!(reader.bytes, b"");
        }
    }
}

#[test]
fn every_short_frame_refuses_and_trailing_data_consumes_only_one_probe_byte() {
    for response in responses() {
        let mut frame = encode_response(&response, &NONCE).unwrap();
        for end in 0..frame.len() {
            assert_eq!(
                read_closed_response(&mut &frame[..end], &NONCE),
                Err(ResponseError::Framing)
            );
        }
        frame.extend_from_slice(b"private trailing data");
        let mut reader = frame.as_slice();
        assert_eq!(
            read_closed_response(&mut reader, &NONCE),
            Err(ResponseError::Framing)
        );
        assert_eq!(reader, b"rivate trailing data");
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
        Err(io::Error::other("private body transport detail"))
    }
}

#[test]
fn invalid_lengths_never_access_body_and_maximum_valid_length_stays_bounded() {
    for length in [0_u32, 1_048_577, u32::MAX] {
        let header = length.to_le_bytes();
        let mut reader = BodyProbe {
            header: &header,
            requested: vec![],
        };
        assert_eq!(
            read_closed_response(&mut reader, &NONCE),
            Err(ResponseError::Framing)
        );
        assert_eq!(reader.requested, Vec::<usize>::new());
    }
    for length in [1_u32, 1_048_575, 1_048_576] {
        let header = length.to_le_bytes();
        let mut reader = BodyProbe {
            header: &header,
            requested: vec![],
        };
        assert_eq!(
            read_closed_response(&mut reader, &NONCE),
            Err(ResponseError::Framing)
        );
        assert_eq!(reader.requested, vec![length as usize]);
    }
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
fn io_failures_at_header_body_and_eof_have_only_static_diagnostics() {
    for response in responses() {
        let frame = encode_response(&response, &NONCE).unwrap();
        for end in [0, 2, 4, frame.len() - 1, frame.len()] {
            let error = read_closed_response(&mut FailAfter(&frame[..end]), &NONCE).unwrap_err();
            assert_eq!(error, ResponseError::Framing);
            assert_eq!(error.to_string(), "invalid console response framing");
        }
    }
}

#[test]
fn stream_preserves_codec_payload_shape_and_correlation_refusals() {
    let response = responses().remove(0);
    let frame = encode_response(&response, &NONCE).unwrap();
    assert_eq!(
        read_closed_response(&mut frame.as_slice(), &[8; 16]),
        Err(ResponseError::Admission)
    );
    let original = std::str::from_utf8(&frame[4..]).unwrap();
    for payload in [
        "!".to_string(),
        original.replacen("\"version\":1", "\"version\":2", 1),
        original.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
        original.replacen("\"workerPid\":1", "\"workerPid\":0", 1),
        original.replacen("\"workerPid\":1", "\"workerPid\":4294967296", 1),
        original.replacen("\"type\":\"ready\"", "\"type\":[\"ready\"]", 1),
    ] {
        let mut malformed = (payload.len() as u32).to_le_bytes().to_vec();
        malformed.extend_from_slice(payload.as_bytes());
        let expected = decode_response(&malformed, &NONCE);
        assert!(expected.is_err());
        assert_eq!(
            read_closed_response(&mut malformed.as_slice(), &NONCE),
            expected
        );
    }
}
