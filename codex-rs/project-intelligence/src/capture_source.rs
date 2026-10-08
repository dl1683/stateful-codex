//! Host-owned original text parts. A seal proves observed bytes, never endorsement.
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceSpanRole {
    Body,
    Heading,
    Attribution,
    Qualifier,
    Duration,
    Scope,
    Evidence,
    Temporal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub role: SourceSpanRole,
}

/// Only the host ingress adapter constructs observations; there is no model tool for this.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceObservation {
    pub project_id: String,
    pub authoritative_thread_id: String,
    pub binding_generation: u64,
    pub original_event_id: String,
    pub turn_id: String,
    pub part_index: u32,
    pub source_revision: u64,
    pub complete_envelope: bool,
    pub incomplete_reason: Option<String>,
    pub ordered_spans: Vec<SourceSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSeal {
    pub observation: SourceObservation,
    pub origin: SourceOrigin,
    pub original_utf8_length: u32,
    pub digest: String,
    pub immutable_first_observation_sequence: u64,
    pub recorded_at_ms: i64,
    pub capture_contract_version: String,
    pub exact_source_locator: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceOrigin {
    LiveUserTurnText,
}

/// One exact range page. Offsets always address the original part, including CRLF.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRangeRead {
    pub seal: SourceSeal,
    pub start_byte: u32,
    pub end_byte: u32,
    pub exact_text: String,
    pub next_offset: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSearchPage {
    pub ranges: Vec<SourceRangeRead>,
    pub after: Option<SourceSearchCursor>,
    pub examined: u32,
    pub complete: bool,
}

/// Pins query, project, source/index generation and eligibility revision across pages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSearchCursor {
    pub project_id: String,
    pub query_digest: String,
    pub source_watermark: i64,
    pub eligibility_revision: i64,
    pub index_generation: i64,
    pub after_source: String,
    pub after_byte: u32,
}

/// Exact evidence route, never a synthesized quotation or admission assertion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLink {
    pub locator: String,
    pub digest: String,
    pub source_revision: u64,
    pub span: SourceSpan,
}

/// Evidence pagination pins the entry and link set; changed evidence restarts discovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLinkCursor {
    pub project_id: String,
    pub entry_id: String,
    pub entry_revision: u64,
    pub link_count: i64,
    pub last: SourceLink,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLinkPage {
    pub links: Vec<SourceLink>,
    pub after: Option<SourceLinkCursor>,
    pub complete: bool,
}

impl SourceObservation {
    pub(crate) fn validate(&self, text: &str) -> Result<(), crate::BlackboardStoreError> {
        let bounded_id = |id: &str| !id.is_empty() && id.len() <= 512;
        if ![
            &self.project_id,
            &self.authoritative_thread_id,
            &self.original_event_id,
            &self.turn_id,
        ]
        .into_iter()
        .all(|id| bounded_id(id))
            || self.binding_generation == 0
            || self.binding_generation > i64::MAX as u64
            || self.source_revision == 0
            || self.source_revision > i64::MAX as u64
            || text.is_empty()
            || text.len() > 65536
            || self.ordered_spans.len() > 8
            || self
                .incomplete_reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 240)
            || self.complete_envelope == self.incomplete_reason.is_some()
        {
            return Err(crate::BlackboardStoreError::InvalidSource);
        }
        let mut previous = 0;
        let mut bytes = 0;
        for span in &self.ordered_spans {
            let start = span.start_byte as usize;
            let end = span.end_byte as usize;
            if start < previous
                || start >= end
                || end > text.len()
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return Err(crate::BlackboardStoreError::InvalidSource);
            }
            previous = end;
            bytes += end - start;
        }
        if self.complete_envelope && (bytes == 0 || bytes > 16384) {
            return Err(crate::BlackboardStoreError::InvalidSource);
        }
        Ok(())
    }
}
