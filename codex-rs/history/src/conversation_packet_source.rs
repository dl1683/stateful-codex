//! Bounded selection of original deliveries for a compaction packet.
//!
//! The host supplies the history snapshot taken before any compactor mutation. Only host
//! metadata classifies a delivery: a retained source, a persisted delivery order and an origin
//! thread. Positions are delivery orders, never vector indices. Text is copied only after the
//! bounded window is chosen, and anything the host cannot vouch for lowers the packet's input
//! coverage instead of being guessed.

use codex_protocol::ThreadId;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;

use crate::ConversationInputCoverage;
use crate::ConversationPacket;
use crate::ConversationPacketBoundary;
use crate::ConversationPacketBudget;
use crate::ConversationPacketError;
use crate::ConversationPacketInput;
use crate::ConversationPacketRecord;
use crate::ConversationPacketSize;
use crate::ConversationRecordKind;
use crate::MAX_PACKET_CANDIDATE_BYTES;
use crate::MAX_PACKET_CANDIDATES;
use crate::ResponseItemEnvelope;
use crate::RetainedSource;
use crate::RetainedSourceRole;
use crate::pack_conversation_packet;

/// Ceiling on the stored packet, measured as its serialized size until rendering exists.
const PACKET_BUDGET: ConversationPacketBudget = ConversationPacketBudget {
    max_rendered_tokens: 2_048,
    max_rendered_bytes: 8 * 1024,
    max_records: MAX_PACKET_CANDIDATES,
};

/// Whether the snapshot still holds every delivery since the thread started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryContinuity {
    SinceThreadStart,
    Rewritten,
}

/// One thread's history as captured before a compaction mutates it.
pub struct ConversationPacketSource<'a> {
    pub thread_id: ThreadId,
    pub history: &'a [ResponseItemEnvelope],
    pub continuity: HistoryContinuity,
}

struct Candidate<'a> {
    origin: ThreadId,
    source: &'a RetainedSource,
    sequence: u64,
    kind: ConversationRecordKind,
    content: &'a [ContentItem],
}

impl Candidate<'_> {
    fn parts(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                Some(text.as_str())
            }
            _ => None,
        })
    }

    /// Joined length, matching how the host retains multi-part messages. Incomplete sources
    /// are offered only to be counted; their text is never packed.
    fn offered_bytes(&self) -> usize {
        if !self.source.complete {
            return 0;
        }
        self.parts()
            .map(|part| part.len() + 1)
            .sum::<usize>()
            .saturating_sub(1)
    }

    fn into_record(self) -> ConversationPacketRecord {
        let text = if self.source.complete {
            self.parts().collect::<Vec<_>>().join("\n")
        } else {
            String::new()
        };
        ConversationPacketRecord::new(
            self.origin,
            self.source.clone(),
            self.sequence,
            self.kind,
            text,
        )
    }
}

#[derive(Default)]
struct Gaps {
    missing_provenance: bool,
    bounded: bool,
}

/// Assembles the packet for a compaction checkpoint, or `None` when there is no evidence at all.
pub fn assemble_conversation_packet(
    source: ConversationPacketSource<'_>,
) -> Result<Option<ConversationPacket>, ConversationPacketError> {
    let mut gaps = Gaps::default();
    let candidates = history_candidates(source.history, &mut gaps);
    // A shortened or partly non-text source is a known omission from the evidence.
    gaps.bounded |= candidates
        .iter()
        .any(|candidate| !candidate.source.complete);
    let Some(through_sequence) = candidates.iter().map(|candidate| candidate.sequence).max() else {
        return Ok(None);
    };
    let candidates = admit_by_priority(candidates, &mut gaps);

    let input_coverage = if gaps.missing_provenance {
        ConversationInputCoverage::Unknown
    } else if gaps.bounded || source.continuity == HistoryContinuity::Rewritten {
        ConversationInputCoverage::BoundedWindow
    } else {
        ConversationInputCoverage::CompleteThroughCutoff
    };
    pack_conversation_packet(
        ConversationPacketInput {
            boundary: ConversationPacketBoundary::new(source.thread_id, through_sequence),
            input_coverage,
            previous: None,
            candidates: &candidates,
        },
        PACKET_BUDGET,
        measure,
    )
    .map(Some)
}

/// Chooses the offered window with the packer's own priorities, so no older final is displaced
/// by newer commentary: the latest final and the request before it, other finals, then users,
/// each newest first; commentary takes only leftover room, and incomplete sources come last.
/// Whole records are skipped, never shortened, and text is copied only for the admitted window.
fn admit_by_priority(
    mut candidates: Vec<Candidate<'_>>,
    gaps: &mut Gaps,
) -> Vec<ConversationPacketRecord> {
    let latest = |kind: ConversationRecordKind, before: u64| {
        candidates
            .iter()
            .filter(|candidate| {
                candidate.source.complete && candidate.kind == kind && candidate.sequence < before
            })
            .map(|candidate| candidate.sequence)
            .max()
    };
    let latest_final = latest(ConversationRecordKind::AssistantFinal, u64::MAX);
    let request = latest_final
        .and_then(|final_sequence| latest(ConversationRecordKind::User, final_sequence));
    candidates.sort_by_key(|candidate| {
        let tier = match (candidate.source.complete, candidate.kind) {
            (false, _) => 5,
            (true, ConversationRecordKind::AssistantFinal)
                if Some(candidate.sequence) == latest_final =>
            {
                0
            }
            (true, ConversationRecordKind::User) if Some(candidate.sequence) == request => 1,
            (true, ConversationRecordKind::AssistantFinal) => 2,
            (true, ConversationRecordKind::User) => 3,
            (true, ConversationRecordKind::AssistantCommentary) => 4,
        };
        (tier, std::cmp::Reverse(candidate.sequence))
    });

    let (mut admitted, mut bytes) = (Vec::new(), 0);
    for candidate in candidates {
        if admitted.len() == MAX_PACKET_CANDIDATES
            || bytes + candidate.offered_bytes() > MAX_PACKET_CANDIDATE_BYTES
        {
            gaps.bounded = true;
            continue;
        }
        bytes += candidate.offered_bytes();
        admitted.push(candidate.into_record());
    }
    admitted
}

/// Original messages the host captured as retained sources, in their recorded form.
fn history_candidates<'a>(
    history: &'a [ResponseItemEnvelope],
    gaps: &mut Gaps,
) -> Vec<Candidate<'a>> {
    let mut candidates = Vec::new();
    for envelope in history {
        let ResponseItem::Message {
            role,
            content,
            phase,
            ..
        } = &envelope.item
        else {
            continue;
        };
        let metadata = envelope.metadata.as_ref();
        if metadata.is_some_and(|metadata| metadata.compaction_output) {
            continue;
        }
        let Some((metadata, source)) = metadata.and_then(|metadata| {
            metadata
                .retained_source
                .as_ref()
                .map(|source| (metadata, source))
        }) else {
            // Every assistant message is a delivery; user-role context fragments are not.
            gaps.missing_provenance |= role == "assistant";
            continue;
        };
        if metadata.inherited_user_message {
            // Copied parent context has no position in this thread's delivery order.
            gaps.bounded = true;
            continue;
        }
        let (Some(origin), Some(sequence)) = (
            metadata.conversation_origin_thread_id,
            metadata.user_input_order,
        ) else {
            gaps.missing_provenance = true;
            continue;
        };
        candidates.push(Candidate {
            origin,
            source,
            sequence,
            kind: record_kind(source.id.role, phase.as_ref()),
            content,
        });
    }
    candidates
}

/// Host classification only: an absent or commentary phase never becomes a final answer.
fn record_kind(role: RetainedSourceRole, phase: Option<&MessagePhase>) -> ConversationRecordKind {
    match (role, phase) {
        (RetainedSourceRole::User, _) => ConversationRecordKind::User,
        (RetainedSourceRole::Assistant, Some(MessagePhase::FinalAnswer)) => {
            ConversationRecordKind::AssistantFinal
        }
        (RetainedSourceRole::Assistant, Some(MessagePhase::Commentary) | None) => {
            ConversationRecordKind::AssistantCommentary
        }
    }
}

/// Deterministic size of the stored sidecar; rendering may later supply a tighter measure.
fn measure(packet: &ConversationPacket) -> ConversationPacketSize {
    let rendered_bytes = serde_json::to_vec(packet).map_or(usize::MAX, |bytes| bytes.len());
    ConversationPacketSize {
        rendered_tokens: rendered_bytes.div_ceil(4),
        rendered_bytes,
    }
}

#[cfg(test)]
#[path = "conversation_packet_source_tests.rs"]
mod tests;
