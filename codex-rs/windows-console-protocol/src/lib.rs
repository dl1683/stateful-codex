//! Inert Windows console-worker request framing. Decoded references are non-owning
//! data, not authenticated capabilities, launch authority or lifecycle evidence.

mod codec;
mod request;
mod stream;

pub use codec::ProtocolError;
pub use codec::decode_request;
pub use codec::encode_request;
pub use request::HandleReferences;
pub use request::OutputMode;
pub use request::ReceiveContext;
pub use request::Request;
pub use request::WorkerWidth;
pub use stream::read_closed_request;

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
