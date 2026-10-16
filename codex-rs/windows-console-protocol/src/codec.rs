use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::OutputMode;
use crate::ReceiveContext;
use crate::Request;
use crate::WorkerWidth;

const MAX_PAYLOAD: usize = 1024 * 1024;

/// Bounded stage-only errors; never echo command/environment or parser text.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProtocolError {
    #[error("invalid console request framing")]
    Framing,
    #[error("invalid console request payload")]
    Payload,
    #[error("console request correlation or shape refused")]
    Admission,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    #[serde(rename = "type")]
    direction: Direction,
    request: Request,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Direction {
    Request,
}

fn validate(request: &Request, context: &ReceiveContext) -> Result<(), ProtocolError> {
    if request.nonce != context.nonce {
        return Err(ProtocolError::Admission);
    }
    for (value, cap) in [
        (&request.application, 32767),
        (&request.command_line, 32766),
        (&request.cwd, 32767),
        (&request.startup_desktop, 1024),
    ] {
        if value.is_empty() || value.len() > cap || value.contains(&0) {
            return Err(ProtocolError::Admission);
        }
    }
    let environment = &request.environment_block;
    if !(2..=65536).contains(&environment.len())
        || !environment.ends_with(&[0, 0])
        || environment.len() > 2 && environment[0] == 0
        || environment[..environment.len() - 1]
            .windows(/*size*/ 2)
            .any(|pair| pair == [0, 0])
    {
        return Err(ProtocolError::Admission);
    }
    let handles = &request.handles;
    let protected = [
        handles.restricted_token,
        handles.inner_job,
        context.request_pipe,
        context.status_pipe,
    ];
    let stdio = [handles.stdin, handles.stdout, handles.stderr];
    for value in protected.into_iter().chain(stdio) {
        if matches!(value, 0 | u64::MAX)
            || value == u64::from(u32::MAX)
            || matches!(context.width, WorkerWidth::Bits32) && value > u64::from(u32::MAX)
        {
            return Err(ProtocolError::Admission);
        }
    }
    for (index, value) in protected.iter().enumerate() {
        if protected[..index].contains(value) || stdio.contains(value) {
            return Err(ProtocolError::Admission);
        }
    }
    if handles.stdin == handles.stdout
        || handles.stdin == handles.stderr
        || match request.output_mode {
            OutputMode::Separate => handles.stdout == handles.stderr,
            OutputMode::Merged => handles.stdout != handles.stderr,
        }
    {
        return Err(ProtocolError::Admission);
    }
    Ok(())
}

/// Validate field bounds before serialization; no launch or owning handles.
pub fn encode_request(
    request: &Request,
    context: &ReceiveContext,
) -> Result<Vec<u8>, ProtocolError> {
    validate(request, context)?;
    let payload = serde_json::to_vec(&Envelope {
        version: 1,
        direction: Direction::Request,
        request: request.clone(),
    })
    .map_err(|_| ProtocolError::Payload)?;
    if payload.len() > MAX_PAYLOAD {
        return Err(ProtocolError::Framing);
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode exactly one closed frame; reject trailing bytes and other directions.
pub fn decode_request(frame: &[u8], context: &ReceiveContext) -> Result<Request, ProtocolError> {
    let header: [u8; 4] = frame
        .get(..4)
        .ok_or(ProtocolError::Framing)?
        .try_into()
        .map_err(|_| ProtocolError::Framing)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_PAYLOAD || frame.len() != length + 4 {
        return Err(ProtocolError::Framing);
    }
    let envelope: Envelope =
        serde_json::from_slice(&frame[4..]).map_err(|_| ProtocolError::Payload)?;
    if envelope.version != 1 {
        return Err(ProtocolError::Payload);
    }
    validate(&envelope.request, context)?;
    Ok(envelope.request)
}
