//! Exact delivered conversation records carried across a compaction boundary.
//!
//! A packet holds whole original user and assistant messages, never rewritten or shortened
//! text, so the agent can quote what it actually said after the provider's opaque compaction
//! has dropped the visible messages. Selection is budgeted by the host's real rendering;
//! anything left out is counted in the packet's coverage rather than silently lost.

use std::collections::HashMap;
use std::fmt;

use codex_protocol::ThreadId;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::RetainedSource;
use crate::RetainedSourceRole;

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

/// Selects whole records for a compaction packet within the host-measured budget.
///
/// Priority: the latest final answer and the user request before it, then other final answers
/// newest first, then user messages newest first, then commentary. A record that does not fit is
/// skipped in favor of later ones rather than truncated. Every measurement sees the coverage the
/// packet would carry, and the returned packet is measured in its final form. Output is in
/// branch order.
pub fn pack_conversation_packet(
    input: ConversationPacketInput<'_>,
    budget: ConversationPacketBudget,
    mut measure: impl FnMut(&ConversationPacket) -> ConversationPacketSize,
) -> Result<ConversationPacket, ConversationPacketError> {
    let previous = input
        .previous
        .map_or(&[][..], |packet| packet.records.as_slice());
    let offered = previous.iter().chain(input.candidates);
    let records = previous.len() + input.candidates.len();
    let bytes: usize = offered.clone().map(|record| record.text.len()).sum();
    if records > MAX_PACKET_CANDIDATES || bytes > MAX_PACKET_CANDIDATE_BYTES {
        return Err(ConversationPacketError::InputBoundExceeded { records, bytes });
    }

    let (admitted, omitted_incomplete) = admit(offered, input.boundary.through_sequence)?;
    let mut packet = ConversationPacket {
        version: CONVERSATION_PACKET_VERSION,
        boundary: input.boundary,
        records: Vec::new(),
        coverage: ConversationPacketCoverage {
            input_coverage: input.input_coverage,
            considered_records: admitted.len(),
            included_records: 0,
            omitted_by_budget: admitted.len(),
            omitted_incomplete,
        },
    };
    if !fits(&measure(&packet), budget) {
        return Err(ConversationPacketError::HeaderExceedsBudget);
    }

    // `chosen` keeps the included records in priority order so the least important can be
    // dropped first if the final coverage no longer fits.
    let mut chosen: Vec<&ConversationPacketRecord> = Vec::new();
    for record in priority_order(&admitted) {
        if chosen.len() >= budget.max_records {
            continue;
        }
        chosen.push(record);
        set_records(&mut packet, &chosen, admitted.len());
        if !fits(&measure(&packet), budget) {
            chosen.pop();
            set_records(&mut packet, &chosen, admitted.len());
        }
    }
    while !fits(&measure(&packet), budget) {
        if chosen.pop().is_none() {
            return Err(ConversationPacketError::HeaderExceedsBudget);
        }
        set_records(&mut packet, &chosen, admitted.len());
    }
    Ok(packet)
}

type RecordIdentity<'a> = (ThreadId, &'a RetainedSource);

/// Validates offered records and removes duplicates, keeping the first delivery of each source
/// revision. A repeated revision must match exactly; anything else means the host's metadata is
/// inconsistent and nothing is packed.
fn admit<'a>(
    offered: impl Iterator<Item = &'a ConversationPacketRecord>,
    through_sequence: u64,
) -> Result<(Vec<&'a ConversationPacketRecord>, usize), ConversationPacketError> {
    let mut admitted: Vec<&ConversationPacketRecord> = Vec::new();
    let mut seen: HashMap<RecordIdentity<'a>, &ConversationPacketRecord> = HashMap::new();
    let mut omitted_incomplete = 0;
    for record in offered {
        if record.sequence > through_sequence {
            return Err(ConversationPacketError::AfterCutoff {
                sequence: record.sequence,
                through_sequence,
            });
        }
        if record.source.id.role != source_role(record.kind) {
            return Err(ConversationPacketError::InvalidSourceKind {
                sequence: record.sequence,
            });
        }
        if !record.source.complete {
            omitted_incomplete += 1;
            continue;
        }
        match seen.get(&(record.origin_thread_id, &record.source)) {
            Some(first) if *first == record => {}
            Some(_) => {
                return Err(ConversationPacketError::ConflictingSource {
                    sequence: record.sequence,
                });
            }
            None => {
                seen.insert((record.origin_thread_id, &record.source), record);
                admitted.push(record);
            }
        }
    }
    Ok((admitted, omitted_incomplete))
}

fn source_role(kind: ConversationRecordKind) -> RetainedSourceRole {
    match kind {
        ConversationRecordKind::User => RetainedSourceRole::User,
        ConversationRecordKind::AssistantFinal | ConversationRecordKind::AssistantCommentary => {
            RetainedSourceRole::Assistant
        }
    }
}

fn set_records(
    packet: &mut ConversationPacket,
    chosen: &[&ConversationPacketRecord],
    considered: usize,
) {
    let mut records: Vec<ConversationPacketRecord> =
        chosen.iter().map(|record| (*record).clone()).collect();
    records.sort_by(|left, right| branch_order(left).cmp(&branch_order(right)));
    packet.records = records;
    packet.coverage.included_records = chosen.len();
    packet.coverage.omitted_by_budget = considered - chosen.len();
}

fn fits(size: &ConversationPacketSize, budget: ConversationPacketBudget) -> bool {
    size.rendered_tokens <= budget.max_rendered_tokens
        && size.rendered_bytes <= budget.max_rendered_bytes
}

/// Branch order with a complete identity tie-break, so equal positions sort deterministically.
fn branch_order(record: &ConversationPacketRecord) -> (u64, String, &str, &str, &str) {
    (
        record.sequence,
        record.origin_thread_id.to_string(),
        record.source.id.message_id.as_str(),
        record.source.id.turn_id.as_str(),
        record.source.revision.as_str(),
    )
}

fn priority_order<'a>(
    admitted: &[&'a ConversationPacketRecord],
) -> Vec<&'a ConversationPacketRecord> {
    let newest_first = |kind: ConversationRecordKind| {
        let mut records: Vec<&'a ConversationPacketRecord> = admitted
            .iter()
            .copied()
            .filter(|record| record.kind == kind)
            .collect();
        records.sort_by(|left, right| branch_order(right).cmp(&branch_order(left)));
        records
    };
    let finals = newest_first(ConversationRecordKind::AssistantFinal);
    let users = newest_first(ConversationRecordKind::User);
    let commentary = newest_first(ConversationRecordKind::AssistantCommentary);
    let mut ordered: Vec<&'a ConversationPacketRecord> = Vec::with_capacity(admitted.len());
    if let Some(latest) = finals.first() {
        ordered.push(*latest);
        if let Some(request) = users.iter().find(|user| user.sequence < latest.sequence) {
            ordered.push(*request);
        }
    }
    for record in finals.iter().chain(&users).chain(&commentary) {
        if !ordered.iter().any(|chosen| std::ptr::eq(*chosen, *record)) {
            ordered.push(*record);
        }
    }
    ordered
}

#[cfg(test)]
#[path = "conversation_packet_tests.rs"]
mod tests;
