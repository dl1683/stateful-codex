//! Inert Windows console-worker framing. Decoded references and responses are
//! data, not authenticated capabilities, launch authority or lifecycle evidence.

mod codec;
mod request;
mod response;
mod response_stream;
mod sequence;
mod stream;

pub use codec::ProtocolError;
pub use codec::decode_request;
pub use codec::encode_request;
pub use request::HandleReferences;
pub use request::OutputMode;
pub use request::ReceiveContext;
pub use request::Request;
pub use request::WorkerWidth;
pub use response::FailureStage;
pub use response::Response;
pub use response::ResponseError;
pub use response::ResponseEvent;
pub use response::decode_response;
pub use response::encode_response;
pub use response_stream::read_closed_response;
pub use response_stream::read_response_frame;
pub use sequence::SequenceError;
pub use sequence::WorkerResponseSequence;
pub use stream::read_closed_request;

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
