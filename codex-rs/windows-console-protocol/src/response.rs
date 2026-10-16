//! Inert worker assertions. Correlation is not authentication, lifecycle, drain
//! or authority to preserve/terminate either job, even after RootExited.
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::codec::payload_length;

/// Closed worker failure phases; no unbounded diagnostic payload crosses IPC.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureStage {
    Bootstrap,
    Console,
    HandleAdmission,
    Spawn,
    Control,
    Internal,
}

/// Unauthenticated assertions only. Root exit does not certify child drain or
/// paired-job preservation; the future parent must establish those locally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseEvent {
    Ready {
        worker_pid: u32,
    },
    Spawned {
        target_pid: u32,
    },
    RootExited {
        exit_code: u32,
    },
    Failed {
        stage: FailureStage,
        win32_code: u32,
    },
}

/// Correlated portable data, with no process/handle ownership or OS operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub nonce: [u8; 16],
    pub event: ResponseEvent,
}

/// Static response-stage errors, never raw parser text or payload content.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ResponseError {
    #[error("invalid console response framing")]
    Framing,
    #[error("invalid console response payload")]
    Payload,
    #[error("console response correlation or shape refused")]
    Admission,
}

#[derive(Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Wire {
    Ready {
        version: u32,
        nonce: [u8; 16],
        worker_pid: u32,
    },
    Spawned {
        version: u32,
        nonce: [u8; 16],
        target_pid: u32,
    },
    RootExited {
        version: u32,
        nonce: [u8; 16],
        exit_code: u32,
    },
    Failed {
        version: u32,
        nonce: [u8; 16],
        stage: FailureStage,
        win32_code: u32,
    },
}

fn validate(response: &Response, nonce: &[u8; 16]) -> Result<(), ResponseError> {
    if &response.nonce != nonce
        || matches!(
            response.event,
            ResponseEvent::Ready { worker_pid: 0 } | ResponseEvent::Spawned { target_pid: 0 }
        )
    {
        return Err(ResponseError::Admission);
    }
    Ok(())
}

/// Encode one closed response frame. PIDs are nonzero; all u32 exit/error bits
/// are retained. The nonce must match trusted correlation input, not sender trust.
pub fn encode_response(response: &Response, nonce: &[u8; 16]) -> Result<Vec<u8>, ResponseError> {
    validate(response, nonce)?;
    let nonce = response.nonce;
    let wire = match response.event {
        ResponseEvent::Ready { worker_pid } => Wire::Ready {
            version: 1,
            nonce,
            worker_pid,
        },
        ResponseEvent::Spawned { target_pid } => Wire::Spawned {
            version: 1,
            nonce,
            target_pid,
        },
        ResponseEvent::RootExited { exit_code } => Wire::RootExited {
            version: 1,
            nonce,
            exit_code,
        },
        ResponseEvent::Failed { stage, win32_code } => Wire::Failed {
            version: 1,
            nonce,
            stage,
            win32_code,
        },
    };
    let payload = serde_json::to_vec(&wire).map_err(|_| ResponseError::Payload)?;
    let header = u32::try_from(payload.len())
        .map_err(|_| ResponseError::Framing)?
        .to_le_bytes();
    payload_length(header).map_err(|_| ResponseError::Framing)?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode exactly one closed response, without accepting local lifecycle inputs
/// or validating sequence/OS effects. Reject duplicate fields from original bytes.
pub fn decode_response(frame: &[u8], nonce: &[u8; 16]) -> Result<Response, ResponseError> {
    let header = frame
        .get(..4)
        .ok_or(ResponseError::Framing)?
        .try_into()
        .map_err(|_| ResponseError::Framing)?;
    let length = payload_length(header).map_err(|_| ResponseError::Framing)?;
    if frame.len() != 4 + length {
        return Err(ResponseError::Framing);
    }
    let shape: serde_json::Value =
        serde_json::from_slice(&frame[4..]).map_err(|_| ResponseError::Payload)?;
    if !shape.is_object()
        || !shape.get("type").is_some_and(serde_json::Value::is_string)
        || shape.get("type").and_then(serde_json::Value::as_str) == Some("failed")
            && !shape.get("stage").is_some_and(serde_json::Value::is_string)
    {
        return Err(ResponseError::Payload);
    }
    let wire: Wire = serde_json::from_slice(&frame[4..]).map_err(|_| ResponseError::Payload)?;
    let (version, response) = match wire {
        Wire::Ready {
            version,
            nonce,
            worker_pid,
        } => (
            version,
            Response {
                nonce,
                event: ResponseEvent::Ready { worker_pid },
            },
        ),
        Wire::Spawned {
            version,
            nonce,
            target_pid,
        } => (
            version,
            Response {
                nonce,
                event: ResponseEvent::Spawned { target_pid },
            },
        ),
        Wire::RootExited {
            version,
            nonce,
            exit_code,
        } => (
            version,
            Response {
                nonce,
                event: ResponseEvent::RootExited { exit_code },
            },
        ),
        Wire::Failed {
            version,
            nonce,
            stage,
            win32_code,
        } => (
            version,
            Response {
                nonce,
                event: ResponseEvent::Failed { stage, win32_code },
            },
        ),
    };
    if version != 1 {
        return Err(ResponseError::Payload);
    }
    validate(&response, nonce)?;
    Ok(response)
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
