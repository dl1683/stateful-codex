//! Host-derived authority for Stateful requests that act as the user.
//!
//! A connection's origin comes from its transport, never from the client. Stdio
//! belongs to the local parent process that launched this server and in-process
//! calls belong to the embedded application; both are host-owned. Network peers
//! (WebSocket and remote control) must not confirm knowledge, steer, or control
//! runs in the user's name. This mirrors the boundary in `user_verification.rs`.

use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCErrorError;

use crate::error_code::invalid_request;
use crate::transport::ConnectionOrigin;

/// Refuses a user-authority Stateful request arriving from a network peer before
/// any processor reads or writes state.
pub(crate) fn require_user_authority(
    request: &ClientRequest,
    origin: ConnectionOrigin,
) -> Result<(), JSONRPCErrorError> {
    let Some(method) = user_authority_method(request) else {
        return Ok(());
    };
    match origin {
        ConnectionOrigin::InProcess | ConnectionOrigin::Stdio => Ok(()),
        ConnectionOrigin::WebSocket | ConnectionOrigin::RemoteControl => {
            Err(invalid_request(format!(
                "{method} acts with the user's authority and is available only to the host-owned stdio or in-process client; nothing was changed"
            )))
        }
    }
}

fn user_authority_method(request: &ClientRequest) -> Option<&'static str> {
    match request {
        ClientRequest::BlackboardConfirm { .. } => Some("blackboard/confirm"),
        ClientRequest::SteeringSubmit { .. } => Some("steering/submit"),
        ClientRequest::StatefulRunStart { .. } => Some("statefulRun/start"),
        ClientRequest::StatefulRunPause { .. } => Some("statefulRun/pause"),
        ClientRequest::StatefulRunResume { .. } => Some("statefulRun/resume"),
        ClientRequest::StatefulRunCancel { .. } => Some("statefulRun/cancel"),
        ClientRequest::StatefulRunSetMode { .. } => Some("statefulRun/setMode"),
        ClientRequest::StatefulMemoryForget { .. } => Some("statefulMemory/forget"),
        ClientRequest::StatefulMemoryCorrect { .. } => Some("statefulMemory/correct"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "stateful_user_authority_tests.rs"]
mod tests;
