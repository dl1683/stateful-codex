//! Bounded selection of original deliveries for a compaction packet.
//!
//! The host supplies the history snapshot taken before any compactor mutation, its retained
//! evidence and the previous live packet. Only host metadata classifies a delivery: a retained
//! source, a persisted delivery order and an origin thread. Positions are delivery orders, never
//! vector indices. Text is copied only after the bounded window is chosen, and anything the
//! host cannot vouch for lowers the packet's input coverage instead of being guessed.

use std::borrow::Cow;

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
use crate::RetainedContext;
use crate::RetainedContextEntry;
use crate::RetainedContextOrder;
use crate::RetainedSource;
use crate::RetainedSourceRole;
use crate::pack_conversation_packet;

/// Ceiling on the stored packet, measured as its serialized size until rendering exists.
const PACKET_BUDGET: ConversationPacketBudget = ConversationPacketBudget {
    max_rendered_tokens: 2_048,
    max_rendered_bytes: 8 * 1024,
    max_records: MAX_PACKET_CANDIDATES,
};
/// Offered slots kept for fresh deliveries, so a full previous packet cannot starve them.
const FRESH_RESERVE: usize = MAX_PACKET_CANDIDATES / 2;

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
    pub retained: &'a RetainedContext,
    pub previous: Option<&'a ConversationPacket>,
    pub continuity: HistoryContinuity,
}

enum CandidateText<'a> {
    Content(&'a [ContentItem]),
    Retained(&'a str),
}

impl CandidateText<'_> {
    fn parts(&self) -> impl Iterator<Item = &str> {
        let (content, retained) = match self {
            Self::Content(content) => (*content, None),
            Self::Retained(text) => (&[][..], Some(*text)),
        };
        content
            .iter()
            .filter_map(|item| match item {
                ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .chain(retained)
    }

    /// Joined length, matching how the host retains multi-part messages.
    fn len(&self) -> usize {
        self.parts()
            .map(|part| part.len() + 1)
            .sum::<usize>()
            .saturating_sub(1)
    }

    fn to_text(&self) -> String {
        self.parts().collect::<Vec<_>>().join("\n")
    }

    /// Compares with joined text without copying it.
    fn matches(&self, text: &str) -> bool {
        let mut rest = text;
        for (index, part) in self.parts().enumerate() {
            let separated = if index == 0 {
                Some(rest)
            } else {
                rest.strip_prefix('\n')
            };
            match separated.and_then(|rest| rest.strip_prefix(part)) {
                Some(remaining) => rest = remaining,
                None => return false,
            }
        }
        rest.is_empty()
    }
}

struct Candidate<'a> {
    origin: ThreadId,
    source: Cow<'a, RetainedSource>,
    sequence: u64,
    kind: ConversationRecordKind,
    text: CandidateText<'a>,
}

impl Candidate<'_> {
    /// Incomplete sources are offered only to be counted; their text is never packed.
    fn offered_bytes(&self) -> usize {
        if self.source.complete {
            self.text.len()
        } else {
            0
        }
    }

    fn into_record(self) -> ConversationPacketRecord {
        let text = if self.source.complete {
            self.text.to_text()
        } else {
            String::new()
        };
        ConversationPacketRecord::new(
            self.origin,
            self.source.into_owned(),
            self.sequence,
            self.kind,
            text,
        )
    }

    fn carried_by(&self, previous: &[ConversationPacketRecord]) -> bool {
        previous.iter().any(|record| {
            record.origin_thread_id() == self.origin
                && record.source() == self.source.as_ref()
                && record.sequence() == self.sequence
                && record.kind() == self.kind
                && self.text.matches(record.text())
        })
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
    let previous = source.previous.map_or(&[][..], ConversationPacket::records);
    let mut gaps = Gaps::default();
    let mut fresh = history_candidates(source.history, &mut gaps);
    fresh.extend(retained_candidates(&source, &fresh, previous, &mut gaps));
    fresh.retain(|candidate| !candidate.carried_by(previous));
    // A shortened or partly non-text source is a known omission from the evidence.
    gaps.bounded |= fresh.iter().any(|candidate| !candidate.source.complete);
    if fresh.is_empty() && previous.is_empty() {
        return Ok(None);
    }
    let through_sequence = fresh
        .iter()
        .map(|candidate| candidate.sequence)
        .chain(
            source
                .previous
                .map(|packet| packet.boundary().through_sequence()),
        )
        .max()
        .unwrap_or_default();

    let candidates = admit_by_priority(previous, fresh, &mut gaps);

    let input_coverage = if gaps.missing_provenance {
        ConversationInputCoverage::Unknown
    } else if gaps.bounded
        || source.continuity == HistoryContinuity::Rewritten
        || source.retained.has_omitted_assistant_messages()
        || source.retained.has_missing_user_messages()
    {
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

/// A record offered to the packer: carried unchanged from the previous packet, or fresh.
enum Offer<'a> {
    Carried(&'a ConversationPacketRecord),
    Fresh(Candidate<'a>),
}

impl Offer<'_> {
    fn kind(&self) -> ConversationRecordKind {
        match self {
            Self::Carried(record) => record.kind(),
            Self::Fresh(candidate) => candidate.kind,
        }
    }

    fn sequence(&self) -> u64 {
        match self {
            Self::Carried(record) => record.sequence(),
            Self::Fresh(candidate) => candidate.sequence,
        }
    }

    fn complete(&self) -> bool {
        match self {
            Self::Carried(record) => record.source().complete,
            Self::Fresh(candidate) => candidate.source.complete,
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Carried(record) => record.text().len(),
            Self::Fresh(candidate) => candidate.offered_bytes(),
        }
    }

    fn into_record(self) -> ConversationPacketRecord {
        match self {
            Self::Carried(record) => record.clone(),
            Self::Fresh(candidate) => candidate.into_record(),
        }
    }
}

/// Chooses the offered window with the packer's own priorities, so no older final is displaced
/// by newer commentary: the latest final and the request before it, other finals, then users,
/// each newest first; commentary takes only leftover room, and incomplete sources come last.
/// Carried records leave room for fresh finals and requests. Whole records are skipped, never
/// shortened, and text is copied only for the admitted window.
fn admit_by_priority(
    carried: &[ConversationPacketRecord],
    fresh: Vec<Candidate<'_>>,
    gaps: &mut Gaps,
) -> Vec<ConversationPacketRecord> {
    let fresh_priority = fresh
        .iter()
        .filter(|candidate| {
            candidate.source.complete
                && candidate.kind != ConversationRecordKind::AssistantCommentary
        })
        .count();
    let carried_cap = MAX_PACKET_CANDIDATES - fresh_priority.min(FRESH_RESERVE);
    let mut offers: Vec<Offer<'_>> = carried
        .iter()
        .map(Offer::Carried)
        .chain(fresh.into_iter().map(Offer::Fresh))
        .collect();
    let latest = |kind: ConversationRecordKind, before: u64| {
        offers
            .iter()
            .filter(|offer| offer.complete() && offer.kind() == kind && offer.sequence() < before)
            .map(Offer::sequence)
            .max()
    };
    let latest_final = latest(ConversationRecordKind::AssistantFinal, u64::MAX);
    let request = latest_final
        .and_then(|final_sequence| latest(ConversationRecordKind::User, final_sequence));
    offers.sort_by_key(|offer| {
        let tier = match (offer.complete(), offer.kind()) {
            (false, _) => 5,
            (true, ConversationRecordKind::AssistantFinal)
                if Some(offer.sequence()) == latest_final =>
            {
                0
            }
            (true, ConversationRecordKind::User) if Some(offer.sequence()) == request => 1,
            (true, ConversationRecordKind::AssistantFinal) => 2,
            (true, ConversationRecordKind::User) => 3,
            (true, ConversationRecordKind::AssistantCommentary) => 4,
        };
        (tier, std::cmp::Reverse(offer.sequence()))
    });

    let (mut admitted, mut carried_admitted, mut bytes) = (Vec::new(), 0, 0);
    for offer in offers {
        let is_carried = matches!(offer, Offer::Carried(_));
        if admitted.len() == MAX_PACKET_CANDIDATES
            || (is_carried && carried_admitted == carried_cap)
            || bytes + offer.bytes() > MAX_PACKET_CANDIDATE_BYTES
        {
            gaps.bounded = true;
            continue;
        }
        bytes += offer.bytes();
        carried_admitted += usize::from(is_carried);
        admitted.push(offer.into_record());
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
            source: Cow::Borrowed(source),
            sequence,
            kind: record_kind(source.id.role, phase.as_ref()),
            text: CandidateText::Content(content),
        });
    }
    candidates
}

/// Retained deliveries that are not in the history snapshot, such as tool-delivered assistant
/// text, at their local acceptance order and with the origin recorded at delivery.
fn retained_candidates<'a>(
    source: &ConversationPacketSource<'a>,
    found: &[Candidate<'a>],
    previous: &[ConversationPacketRecord],
    gaps: &mut Gaps,
) -> Vec<Candidate<'a>> {
    let mut candidates = Vec::new();
    for (order, entry) in source.retained.ordered_entries() {
        let message = match entry {
            RetainedContextEntry::UserMessage(message)
            | RetainedContextEntry::AssistantMessage(message) => message,
            RetainedContextEntry::VerifiedAnswer(_) => continue,
        };
        let RetainedContextOrder::Local(order) = order else {
            gaps.bounded = true;
            continue;
        };
        let Some(retained_source) = source.retained.source(entry) else {
            gaps.missing_provenance = true;
            continue;
        };
        if found
            .iter()
            .any(|candidate| candidate.source.as_ref() == &retained_source)
            || previous
                .iter()
                .any(|record| record.source() == &retained_source)
        {
            continue;
        }
        let Some(origin) = message.origin_thread_id else {
            gaps.missing_provenance = true;
            continue;
        };
        candidates.push(Candidate {
            origin,
            kind: record_kind(retained_source.id.role, message.phase.as_ref()),
            source: Cow::Owned(retained_source),
            sequence: order,
            text: CandidateText::Retained(&message.text),
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
