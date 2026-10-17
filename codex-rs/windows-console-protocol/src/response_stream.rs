use std::io::Read;

use crate::Response;
use crate::ResponseError;
use crate::codec::payload_length;
use crate::decode_response;
use crate::stream::read_closed_frame;

/// Read exactly one required response frame without probing for EOF or another
/// frame at the supplied [`Read`] interface. The existing length cap is checked
/// before allocation/body access; payload and correlation admission are unchanged.
/// Empty input, truncated frames and I/O failures are static framing errors.
///
/// Abandon the stream after any error: this reader cannot resynchronize. The owner
/// must enforce deadlines that unblock a blocking `Read`, decide when EOF is
/// expected, and separately admit event order. A decoded assertion establishes no
/// authentication, process ownership, completion, drain or job-preservation
/// authority, including for `RootExited`. Caller-provided readers may buffer.
pub fn read_response_frame<R: Read + ?Sized>(
    reader: &mut R,
    nonce: &[u8; 16],
) -> Result<Response, ResponseError> {
    let mut header = [0; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| ResponseError::Framing)?;
    let length = payload_length(header).map_err(|_| ResponseError::Framing)?;
    let mut frame = vec![0; 4 + length];
    frame[..4].copy_from_slice(&header);
    reader
        .read_exact(&mut frame[4..])
        .map_err(|_| ResponseError::Framing)?;
    decode_response(&frame, nonce)
}

/// Read one response from a blocking, closed single-frame transport, validating
/// the capped length before allocation/body access and requiring EOF afterward.
/// Transport failures expose only static framing errors. Payload and correlation
/// use the same strict admission as [`decode_response`].
///
/// The sender must close its write end; the owner must enforce a deadline that
/// unblocks the transport. An arbitrary `Read` cannot be cancelled here. This is
/// not a persistent event stream and establishes no authentication, event order,
/// process ownership, exit/drain or job-preservation authority.
pub fn read_closed_response<R: Read + ?Sized>(
    reader: &mut R,
    nonce: &[u8; 16],
) -> Result<Response, ResponseError> {
    let frame = read_closed_frame(reader).map_err(|()| ResponseError::Framing)?;
    decode_response(&frame, nonce)
}

#[cfg(test)]
#[path = "response_stream_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "response_frame_tests.rs"]
mod frame_tests;
