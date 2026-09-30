//! Exact delivered conversation records carried across a compaction boundary.
//!
//! A packet holds whole original user and assistant messages, never rewritten or shortened
//! text, so the agent can quote what it actually said after the provider's opaque compaction
//! has dropped the visible messages. Selection is budgeted by the host's real rendering;
//! anything left out is counted in the packet's coverage rather than silently lost.

use std::fmt;

use codex_protocol::ThreadId;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::RetainedSource;

/// Version of the packet's serialized shape.
pub const CONVERSATION_PACKET_VERSION: u32 = 1;
/// Largest number of offered records (previous packet plus candidates) one packing call accepts.
pub const MAX_PACKET_CANDIDATES: usize = 64;
/// Largest total offered text one packing call accepts, checked before copying any of it.
pub const MAX_PACKET_CANDIDATE_BYTES: usize = 512 * 1024;

/// The last history position a packet may contain, within the host-selected branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationPacketBoundary {
    thread_id: ThreadId,
    through_sequence: u64,
}

impl ConversationPacketBoundary {
    pub fn new(thread_id: ThreadId, through_sequence: u64) -> Self {
        Self {
            thread_id,
            through_sequence,
        }
    }

    pub fn thread_id(&self) -> ThreadId {
        self.thread_id
    }

    pub fn through_sequence(&self) -> u64 {
        self.through_sequence
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationRecordKind {
    User,
    AssistantFinal,
    AssistantCommentary,
}

/// One whole original message with its host-owned identity and position.
///
/// `sequence` is the host-assigned position in the selected branch; the host must assign each
/// delivered message one canonical position and kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationPacketRecord {
    origin_thread_id: ThreadId,
    source: RetainedSource,
    sequence: u64,
    kind: ConversationRecordKind,
    text: String,
}

impl ConversationPacketRecord {
    pub fn new(
        origin_thread_id: ThreadId,
        source: RetainedSource,
        sequence: u64,
        kind: ConversationRecordKind,
        text: String,
    ) -> Self {
        Self {
            origin_thread_id,
            source,
            sequence,
            kind,
            text,
        }
    }

    pub fn origin_thread_id(&self) -> ThreadId {
        self.origin_thread_id
    }

    pub fn source(&self) -> &RetainedSource {
        &self.source
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn kind(&self) -> ConversationRecordKind {
        self.kind
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// How much of the branch's history the host supplied as candidates. This describes the input;
/// even `CompleteThroughCutoff` does not promise that every record fit in the packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationInputCoverage {
    CompleteThroughCutoff,
    BoundedWindow,
    Unknown,
}

/// What one packing call did with the records it was offered.
///
/// `considered_records` counts unique complete records; it always equals `included_records +
/// omitted_by_budget`. `omitted_incomplete` counts shortened sources, before deduplication.
/// Counters describe this call only, not losses accumulated over earlier compactions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationPacketCoverage {
    input_coverage: ConversationInputCoverage,
    considered_records: usize,
    included_records: usize,
    omitted_by_budget: usize,
    omitted_incomplete: usize,
}

impl ConversationPacketCoverage {
    pub fn input_coverage(&self) -> ConversationInputCoverage {
        self.input_coverage
    }

    pub fn considered_records(&self) -> usize {
        self.considered_records
    }

    pub fn included_records(&self) -> usize {
        self.included_records
    }

    pub fn omitted_by_budget(&self) -> usize {
        self.omitted_by_budget
    }

    pub fn omitted_incomplete(&self) -> usize {
        self.omitted_incomplete
    }
}

/// A packet of delivered conversation records. Deserializing establishes shape only; a restored
/// packet must be validated before it is trusted or reused as `previous`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationPacket {
    version: u32,
    boundary: ConversationPacketBoundary,
    records: Vec<ConversationPacketRecord>,
    coverage: ConversationPacketCoverage,
}

impl ConversationPacket {
    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn boundary(&self) -> ConversationPacketBoundary {
        self.boundary
    }

    /// Records in branch order.
    pub fn records(&self) -> &[ConversationPacketRecord] {
        &self.records
    }

    pub fn coverage(&self) -> ConversationPacketCoverage {
        self.coverage
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationPacketBudget {
    pub max_rendered_tokens: usize,
    pub max_rendered_bytes: usize,
    pub max_records: usize,
}

/// The size of the packet as the host actually renders it into model context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConversationPacketSize {
    pub rendered_tokens: usize,
    pub rendered_bytes: usize,
}

pub struct ConversationPacketInput<'a> {
    pub boundary: ConversationPacketBoundary,
    pub input_coverage: ConversationInputCoverage,
    /// The packet installed at the previous compaction, whose records are carried unchanged.
    pub previous: Option<&'a ConversationPacket>,
    pub candidates: &'a [ConversationPacketRecord],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationPacketError {
    InvalidSourceKind {
        sequence: u64,
    },
    ConflictingSource {
        sequence: u64,
    },
    AfterCutoff {
        sequence: u64,
        through_sequence: u64,
    },
    InputBoundExceeded {
        records: usize,
        bytes: usize,
    },
    HeaderExceedsBudget,
}

impl fmt::Display for ConversationPacketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceKind { sequence } => write!(
                formatter,
                "record {sequence} has a source role that does not match its kind"
            ),
            Self::ConflictingSource { sequence } => write!(
                formatter,
                "record {sequence} reuses a source revision with different text, position or kind"
            ),
            Self::AfterCutoff {
                sequence,
                through_sequence,
            } => write!(
                formatter,
                "record {sequence} is after the packet cutoff {through_sequence}"
            ),
            Self::InputBoundExceeded { records, bytes } => write!(
                formatter,
                "packet input of {records} records and {bytes} bytes exceeds the bound"
            ),
            Self::HeaderExceedsBudget => {
                formatter.write_str("the packet header alone exceeds the budget")
            }
        }
    }
}

impl std::error::Error for ConversationPacketError {}
