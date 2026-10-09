//! Model interpretations of exact user-delivered sources, never standing authority.
use crate::SourceSpan;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalCategory {
    Rule,
    Background,
    AttributedContext,
    Decision,
    BrainstormOption,
    /// An approach the user rejected, with the reason they gave.
    RuledOut,
    OpenCheck,
    Note,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalDependency {
    Scope,
    Attribution,
    Referent,
    Promotion,
}

/// Model-declared reading of the enclosure, never a host authorship certification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalAttribution {
    Unknown,
    HistoricalUser,
    ReportedThirdParty,
    ReportedAssistant,
    ImportedMaterial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalTimeForm {
    Point,
    Interval,
    Duration,
    Recurrence,
    UnresolvedRelative,
}

/// Indices into separately labelled verbatim spans of this same sealed part.
/// No model-supplied clock, normalized date, speaker or anchor identity is accepted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalTemporal {
    pub source_time_span: Option<u8>,
    pub event_time_span: Option<u8>,
    pub anchor_span: Option<u8>,
    pub form: ProposalTimeForm,
    pub precision: Option<crate::TimePrecision>,
    pub inclusive_start: Option<bool>,
    pub inclusive_end: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceProposal {
    pub source_id: String,
    pub source_revision: u64,
    pub digest: String,
    pub part_index: u32,
    pub spans: Vec<SourceSpan>,
    pub category: ProposalCategory,
    pub interpretation: String,
    pub dependency: Option<ProposalDependency>,
    pub temporal: Option<ProposalTemporal>,
    pub event_status: Option<crate::EventStatus>,
    pub attribution: Option<ProposalAttribution>,
    pub speaker_span: Option<u8>,
}

impl SourceProposal {
    pub(crate) fn validate_bounds(&self) -> Result<(), crate::BlackboardStoreError> {
        if self.source_id.len() != 64
            || self.digest.len() != 64
            || !self.digest.bytes().all(|ch| ch.is_ascii_hexdigit())
            || self.source_revision == 0
            || self.source_revision > i64::MAX as u64
            || self.spans.is_empty()
            || self.spans.len() > 8
            || self.interpretation.is_empty()
            || self.interpretation.len() > 512
        {
            return Err(crate::BlackboardStoreError::InvalidSource);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProposalStatus {
    Proposed,
    Pending,
    Omitted,
    Refused,
}

/// A committed disposition. Stored is distinct from applied; omitted has no entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalResult {
    pub entry_id: Option<String>,
    pub revision: Option<u64>,
    pub status: ProposalStatus,
    pub source_id: String,
    pub reason: Option<String>,
}

/// Durable origin/meaning in the existing revision-bound context payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalContext {
    pub version: u32,
    pub status: ProposalStatus,
    pub source: SourceProposal,
    pub enclosure: SourceSpan,
}
