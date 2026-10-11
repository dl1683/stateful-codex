//! Host validation of a compaction trailer's extractive spans against the exact final request
//! the trailer was produced from.
//!
//! A span is accepted only when it quotes, byte for byte, an indexed request part that maps to
//! a complete original user or published assistant message accepted no later than the request
//! watermark. Speakers, identities and order come from the host, never from the trailer. A
//! malformed trailer yields the floor (no new records) and never a retry or repair request.
//! Validation proves faithful quotation and attribution, not truth.

use std::collections::BTreeSet;

use codex_extension_api::CompactionExtractionResult;
use codex_extension_api::CompactionRequestSnapshot;
use codex_extension_api::CompactionSourceClass;
use codex_extension_api::CompactionTrailer;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

pub(crate) const SIDECAR_CLOSING: &str = "</capsule_sidecar_v1>";
pub(crate) const MAX_SIDECAR_RECORDS: usize = 32;
pub(crate) const MAX_SIDECAR_SUPERSESSIONS: usize = 4;
pub(crate) const MAX_INDEXED_PARTS: usize = 128;
const MAX_RECORD_SPANS: usize = 4;
const MAX_RECORD_QUOTE_BYTES: usize = 512;

/// The index offered to the model for one attempt, frozen with digests before sending.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FrozenIndex {
    attempt: u32,
    trailer_opening: String,
    /// `q` is the position here; each entry is a `request.parts` position and its digest.
    parts: Vec<(usize, String)>,
}

impl FrozenIndex {
    /// Freezes `selected` request parts in order; out-of-range or excess parts are not indexed.
    pub(crate) fn new(
        request: &CompactionRequestSnapshot,
        trailer_opening: &str,
        selected: &[usize],
    ) -> Self {
        let parts = selected
            .iter()
            .filter_map(|&position| Some((position, sha256(&request.parts.get(position)?.text))))
            .take(MAX_INDEXED_PARTS)
            .collect();
        Self {
            attempt: request.attempt,
            trailer_opening: trailer_opening.to_string(),
            parts,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Speaker {
    User,
    /// A reported claim, never a verified fact or user authority.
    Assistant,
}

/// Host-owned provenance of one accepted span (the request-local part of `SourceSpanV1`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceSpan {
    pub origin_thread_id: Option<String>,
    pub item_id: String,
    pub turn_id: String,
    pub part_index: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub source_part_sha256: String,
    pub selected_span_sha256: String,
    pub source_revision: String,
    pub host_sequence: u64,
    pub speaker: Speaker,
    pub request_item_index: usize,
    pub quote: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpanError {
    Unindexed,
    ChangedPart,
    InvalidRange,
    QuoteMismatch,
    ExcludedSource,
    MissingOrigin,
    ChangedOriginal,
    FutureSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordError {
    Span(SpanError),
    Quota,
    /// A supersession correction not spoken by the user.
    CorrectionSpeaker,
    /// A supersession whose new source is not later than, or is, the old source.
    Order,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloorReason {
    /// The trailer does not belong to this attempt's frozen index or marker.
    StaleIndex,
    Oversize,
    Framing,
    Json,
    Version,
    Quota,
}

/// Field structure shared by the model's proposal (`Span`) and the validated record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Record<S> {
    Diagnosis {
        finding: S,
        subject: S,
        uncertainty: Option<S>,
    },
    Choice {
        choice: S,
        reason: S,
        subject: S,
    },
    Rejection {
        rejected: S,
        reason: S,
        subject: S,
    },
    Measurement {
        value: S,
        subject: S,
        run: Option<S>,
        qualifier: Option<S>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Supersession<S> {
    pub old: S,
    pub new: S,
    pub correction: S,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Span {
    pub q: usize,
    pub start: usize,
    pub end: usize,
    pub quote: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    records: Vec<Record<Span>>,
    supersessions: Vec<Supersession<Span>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SidecarOutcome {
    /// Ordinary native summary without a trailer: no new semantic records.
    Absent,
    /// Structural floor; new semantic coverage is unavailable for this boundary.
    Floor(FloorReason),
    Validated {
        records: Vec<Record<SourceSpan>>,
        supersessions: Vec<Supersession<SourceSpan>>,
        /// Whole proposals dropped, by proposal position.
        rejected_records: Vec<(usize, RecordError)>,
        rejected_supersessions: Vec<(usize, RecordError)>,
    },
}

pub(crate) fn validate_sidecar(
    result: &CompactionExtractionResult,
    index: &FrozenIndex,
) -> SidecarOutcome {
    let trailer = match &result.trailer {
        CompactionTrailer::Absent => return SidecarOutcome::Absent,
        CompactionTrailer::Oversize => return SidecarOutcome::Floor(FloorReason::Oversize),
        CompactionTrailer::Present(trailer) => trailer,
    };
    if index.attempt != result.request.attempt
        || index.trailer_opening != result.preparation.trailer_opening
    {
        return SidecarOutcome::Floor(FloorReason::StaleIndex);
    }
    let envelope = match parse_envelope(trailer, &index.trailer_opening) {
        Ok(envelope) => envelope,
        Err(reason) => return SidecarOutcome::Floor(reason),
    };
    let validator = Validator {
        request: &result.request,
        index,
    };
    let mut records = Vec::new();
    let mut rejected_records = Vec::new();
    for (position, record) in envelope.records.iter().enumerate() {
        match validator.record(record) {
            Ok(record) => records.push(record),
            Err(error) => rejected_records.push((position, error)),
        }
    }
    let mut supersessions = Vec::new();
    let mut rejected_supersessions = Vec::new();
    for (position, proposal) in envelope.supersessions.iter().enumerate() {
        match validator.supersession(proposal) {
            Ok(supersession) => supersessions.push(supersession),
            Err(error) => rejected_supersessions.push((position, error)),
        }
    }
    SidecarOutcome::Validated {
        records,
        supersessions,
        rejected_records,
        rejected_supersessions,
    }
}

/// Exactly `opening`, newline, JSON, newline, closing delimiter, then only whitespace.
fn parse_envelope(trailer: &str, opening: &str) -> Result<Envelope, FloorReason> {
    let body = trailer
        .strip_prefix(opening)
        .and_then(|rest| {
            rest.strip_prefix("\r\n")
                .or_else(|| rest.strip_prefix('\n'))
        })
        .ok_or(FloorReason::Framing)?;
    let closing = body.rfind(SIDECAR_CLOSING).ok_or(FloorReason::Framing)?;
    let (json, tail) = body.split_at(closing);
    if !tail[SIDECAR_CLOSING.len()..].trim().is_empty() || !json.ends_with('\n') {
        return Err(FloorReason::Framing);
    }
    let envelope = serde_json::from_str::<Envelope>(json).map_err(|_| FloorReason::Json)?;
    if envelope.version != 1 {
        return Err(FloorReason::Version);
    }
    if envelope.records.len() > MAX_SIDECAR_RECORDS
        || envelope.supersessions.len() > MAX_SIDECAR_SUPERSESSIONS
    {
        return Err(FloorReason::Quota);
    }
    Ok(envelope)
}

struct Validator<'a> {
    request: &'a CompactionRequestSnapshot,
    index: &'a FrozenIndex,
}

impl Validator<'_> {
    fn record(&self, record: &Record<Span>) -> Result<Record<SourceSpan>, RecordError> {
        let spans = match record {
            Record::Diagnosis {
                finding,
                subject,
                uncertainty,
            } => vec![Some(finding), Some(subject), uncertainty.as_ref()],
            Record::Choice {
                choice,
                reason,
                subject,
            } => vec![Some(choice), Some(reason), Some(subject)],
            Record::Rejection {
                rejected,
                reason,
                subject,
            } => vec![Some(rejected), Some(reason), Some(subject)],
            Record::Measurement {
                value,
                subject,
                run,
                qualifier,
            } => vec![Some(value), Some(subject), run.as_ref(), qualifier.as_ref()],
        };
        let distinct = spans
            .iter()
            .flatten()
            .map(|span| (span.q, span.start, span.end, span.quote.as_str()))
            .collect::<BTreeSet<_>>();
        let quoted_bytes = distinct.iter().map(|span| span.3.len()).sum::<usize>();
        if distinct.len() > MAX_RECORD_SPANS || quoted_bytes > MAX_RECORD_QUOTE_BYTES {
            return Err(RecordError::Quota);
        }
        let span = |span: &Span| self.span(span).map_err(RecordError::Span);
        let optional = |value: &Option<Span>| value.as_ref().map(span).transpose();
        Ok(match record {
            Record::Diagnosis {
                finding,
                subject,
                uncertainty,
            } => Record::Diagnosis {
                finding: span(finding)?,
                subject: span(subject)?,
                uncertainty: optional(uncertainty)?,
            },
            Record::Choice {
                choice,
                reason,
                subject,
            } => Record::Choice {
                choice: span(choice)?,
                reason: span(reason)?,
                subject: span(subject)?,
            },
            Record::Rejection {
                rejected,
                reason,
                subject,
            } => Record::Rejection {
                rejected: span(rejected)?,
                reason: span(reason)?,
                subject: span(subject)?,
            },
            Record::Measurement {
                value,
                subject,
                run,
                qualifier,
            } => Record::Measurement {
                value: span(value)?,
                subject: span(subject)?,
                run: optional(run)?,
                qualifier: optional(qualifier)?,
            },
        })
    }

    fn supersession(
        &self,
        proposal: &Supersession<Span>,
    ) -> Result<Supersession<SourceSpan>, RecordError> {
        let span = |span: &Span| self.span(span).map_err(RecordError::Span);
        let old = span(&proposal.old)?;
        let new = span(&proposal.new)?;
        let correction = span(&proposal.correction)?;
        if correction.speaker != Speaker::User {
            return Err(RecordError::CorrectionSpeaker);
        }
        if old.item_id == new.item_id || new.host_sequence <= old.host_sequence {
            return Err(RecordError::Order);
        }
        Ok(Supersession {
            old,
            new,
            correction,
        })
    }

    fn span(&self, span: &Span) -> Result<SourceSpan, SpanError> {
        let (position, frozen_digest) = self.index.parts.get(span.q).ok_or(SpanError::Unindexed)?;
        let part = self
            .request
            .parts
            .get(*position)
            .ok_or(SpanError::Unindexed)?;
        let part_digest = sha256(&part.text);
        if &part_digest != frozen_digest {
            return Err(SpanError::ChangedPart);
        }
        let text = part.text.as_str();
        if span.start >= span.end
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            return Err(SpanError::InvalidRange);
        }
        let selected = &text[span.start..span.end];
        if selected.as_bytes() != span.quote.as_bytes() {
            return Err(SpanError::QuoteMismatch);
        }
        let speaker = match part.class {
            CompactionSourceClass::User => Speaker::User,
            CompactionSourceClass::Assistant => Speaker::Assistant,
            CompactionSourceClass::CommandOutput
            | CompactionSourceClass::CompactionOutput
            | CompactionSourceClass::HostContext
            | CompactionSourceClass::Inherited => return Err(SpanError::ExcludedSource),
        };
        let origin = part.origin.as_ref().ok_or(SpanError::MissingOrigin)?;
        if !origin.byte_identical {
            return Err(SpanError::ChangedOriginal);
        }
        let (Some(sequence), Some(watermark)) = (origin.sequence, self.request.sequence_watermark)
        else {
            return Err(SpanError::MissingOrigin);
        };
        if sequence > watermark {
            return Err(SpanError::FutureSource);
        }
        Ok(SourceSpan {
            origin_thread_id: origin.origin_thread_id.map(|id| id.to_string()),
            item_id: origin.item_id.clone(),
            turn_id: origin.turn_id.clone(),
            part_index: part.part_index,
            byte_start: span.start,
            byte_end: span.end,
            source_part_sha256: part_digest,
            selected_span_sha256: sha256(selected),
            source_revision: origin.source_revision.clone(),
            host_sequence: sequence,
            speaker,
            request_item_index: part.item_index,
            quote: selected.to_string(),
        })
    }
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
