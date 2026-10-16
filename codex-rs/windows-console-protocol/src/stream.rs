use std::io;
use std::io::Read;

use crate::ProtocolError;
use crate::ReceiveContext;
use crate::Request;
use crate::codec::payload_length;
use crate::decode_request;

/// Read exactly one request from a blocking, closed single-frame transport.
/// Validate length before allocation/body access, then require EOF. Short or
/// failed reads are framing errors; no transport/parser text is exposed.
///
/// The sender must close its write end. The process owner must enforce a deadline
/// and unblock the transport; this function cannot cancel an arbitrary `Read`.
/// This inert decoder neither authenticates handles nor launches a process.
pub fn read_closed_request<R: Read + ?Sized>(
    reader: &mut R,
    context: &ReceiveContext,
) -> Result<Request, ProtocolError> {
    let mut header = [0; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| ProtocolError::Framing)?;
    let length = payload_length(header)?;
    let mut frame = vec![0; 4 + length];
    frame[..4].copy_from_slice(&header);
    reader
        .read_exact(&mut frame[4..])
        .map_err(|_| ProtocolError::Framing)?;
    loop {
        match reader.read(&mut [0; 1]) {
            Ok(0) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Ok(_) | Err(_) => return Err(ProtocolError::Framing),
        }
    }
    decode_request(&frame, context)
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
