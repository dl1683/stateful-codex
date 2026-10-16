use std::io;
use std::io::Read;

use pretty_assertions::assert_eq;

use super::read_closed_request;
use crate::HandleReferences;
use crate::OutputMode;
use crate::ProtocolError;
use crate::ReceiveContext;
use crate::Request;
use crate::WorkerWidth;
use crate::decode_request;
use crate::encode_request;

fn fixture() -> (Request, ReceiveContext) {
    let nonce = [9; 16];
    (
        Request {
            nonce,
            application: vec![65],
            command_line: vec![0x092e, 0x0930, 0x093e, 0x0920, 0x0940, 0xd800],
            cwd: vec![67],
            startup_desktop: vec![68],
            environment_block: vec![69, 61, 70, 0, 0],
            handles: HandleReferences {
                restricted_token: 1,
                inner_job: 2,
                stdin: 3,
                stdout: 4,
                stderr: 5,
            },
            output_mode: OutputMode::Separate,
        },
        ReceiveContext {
            nonce,
            width: WorkerWidth::Bits64,
            request_pipe: 6,
            status_pipe: 7,
        },
    )
}

struct ShortReads<'a> {
    bytes: &'a [u8],
    chunk: usize,
    interrupt_next: bool,
}

impl Read for ShortReads<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.interrupt_next = !self.interrupt_next;
        if !self.interrupt_next {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let count = buffer.len().min(self.chunk).min(self.bytes.len());
        buffer[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        Ok(count)
    }
}

#[test]
fn short_interrupted_reads_preserve_the_complete_lossless_request() {
    let (request, context) = fixture();
    let frame = encode_request(&request, &context).unwrap();
    for chunk in [1, 2, 3, 4, 7, frame.len()] {
        let mut reader = ShortReads {
            bytes: &frame,
            chunk,
            interrupt_next: false,
        };
        assert_eq!(read_closed_request(&mut reader, &context).unwrap(), request);
        assert_eq!(reader.bytes, b"");
    }
}

#[test]
fn every_short_frame_and_trailing_data_refuse() {
    let (request, context) = fixture();
    let mut frame = encode_request(&request, &context).unwrap();
    for end in 0..frame.len() {
        assert_eq!(
            read_closed_request(&mut &frame[..end], &context),
            Err(ProtocolError::Framing)
        );
    }
    frame.extend_from_slice(b"secret trailing data");
    let mut reader = frame.as_slice();
    assert_eq!(
        read_closed_request(&mut reader, &context),
        Err(ProtocolError::Framing)
    );
    // Only the one-byte EOF probe is consumed, never an unbounded drain.
    assert_eq!(reader, b"ecret trailing data");
}

struct HeaderOnly {
    header: &'static [u8],
    body_reads: usize,
}

impl Read for HeaderOnly {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.header.is_empty() {
            self.body_reads += 1;
            return Err(io::Error::other("must not access body"));
        }
        self.header.read(buffer)
    }
}

#[test]
fn invalid_lengths_refuse_before_body_access() {
    let (_, context) = fixture();
    for header in [b"\0\0\0\0", b"\x01\0\x10\0", b"\xff\xff\xff\xff"] {
        let mut reader = HeaderOnly {
            header,
            body_reads: 0,
        };
        assert_eq!(
            read_closed_request(&mut reader, &context),
            Err(ProtocolError::Framing)
        );
        assert_eq!(reader.body_reads, 0);
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
fn transport_errors_in_header_body_and_eof_probe_are_static_framing_errors() {
    let (request, context) = fixture();
    let frame = encode_request(&request, &context).unwrap();
    for end in [0, 2, 4, frame.len() - 1, frame.len()] {
        let error = read_closed_request(&mut FailAfter(&frame[..end]), &context).unwrap_err();
        assert_eq!(error, ProtocolError::Framing);
        assert_eq!(error.to_string(), "invalid console request framing");
    }
}

#[test]
fn closed_stream_delegates_payload_and_admission_checks_to_the_closed_codec() {
    let (request, mut context) = fixture();
    let mut frame = encode_request(&request, &context).unwrap();
    context.nonce[0] += 1;
    assert_eq!(
        read_closed_request(&mut frame.as_slice(), &context),
        Err(ProtocolError::Admission)
    );
    context.nonce[0] -= 1;
    frame[4] = b'!';
    assert_eq!(
        read_closed_request(&mut frame.as_slice(), &context),
        decode_request(&frame, &context)
    );
}
