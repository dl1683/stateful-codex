use std::io::Read;

use crate::Response;
use crate::ResponseError;
use crate::decode_response;
use crate::stream::read_closed_frame;

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
