use serde::Deserialize;
use serde::Serialize;

/// Receiving worker width, independent of the codec host's architecture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerWidth {
    Bits32,
    Bits64,
}

/// Trusted bootstrap correlation inputs, not authentication or OS admission.
pub struct ReceiveContext {
    pub nonce: [u8; 16],
    pub width: WorkerWidth,
    pub request_pipe: u64,
    pub status_pipe: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputMode {
    Separate,
    Merged,
}

/// Worker-local numeric references; parsing never owns or closes a handle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandleReferences {
    pub restricted_token: u64,
    pub inner_job: u64,
    pub stdin: u64,
    pub stdout: u64,
    pub stderr: u64,
}

/// Lossless already-quoted Windows buffers. Integration must separately admit
/// application/desktop identity, token restrictions, handle rights and custody.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub nonce: [u8; 16],
    pub application: Vec<u16>,
    pub command_line: Vec<u16>,
    pub cwd: Vec<u16>,
    pub startup_desktop: Vec<u16>,
    pub environment_block: Vec<u16>,
    pub handles: HandleReferences,
    pub output_mode: OutputMode,
}
