use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::ContextMapEntryId;
use crate::HierarchyNodeId;
use crate::SourceFingerprint;

const MAX_ID_BYTES: usize = 512;
const MAX_PROJECT_ID_BYTES: usize = 512;
const MAX_CONTENT_BYTES: usize = 4_096;
const MAX_STRUCTURED_VALUE_BYTES: usize = 2_048;
const MAX_UNIT_BYTES: usize = 128;
const MAX_EVIDENCE_LINKS: usize = 32;
const MAX_PROVENANCE_SOURCE_BYTES: usize = 512;
const MAX_CONFIDENCE_BASIS_POINTS: u16 = 10_000;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BlackboardEntryId(String);

impl BlackboardEntryId {
    pub fn parse(value: impl Into<String>) -> Result<Self, BlackboardError> {
        let value = value.into();
        validate_identity(&value, MAX_ID_BYTES).map_err(|()| BlackboardError::InvalidEntryId)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlackboardEntryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlackboardKind {
    Instruction,
    Fact,
    Claim,
    Number,
    Decision,
    Strategy,
    Question,
    Contradiction,
    Failure,
    RejectedApproach,
    Signal,
    Note,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlackboardVerification {
    Unverified,
    SourceVerified,
    UserConfirmed,
    Disputed,
    Stale,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlackboardImportance {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootPromotion {
    NotPromoted,
    Candidate,
    Promoted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlackboardEntryState {
    Active,
    Superseded,
    Tombstoned,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlackboardProvenanceKind {
    User,
    Agent,
    Maintenance,
    Import,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConfidenceScore(u16);

impl ConfidenceScore {
    pub fn from_basis_points(value: u16) -> Result<Self, BlackboardError> {
        if value > MAX_CONFIDENCE_BASIS_POINTS {
            return Err(BlackboardError::InvalidConfidence);
        }
        Ok(Self(value))
    }

    pub fn basis_points(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackboardStructuredValue {
    pub value: String,
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackboardEvidenceLink {
    pub context_map_entry_id: ContextMapEntryId,
    pub source_fingerprint: SourceFingerprint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackboardProvenance {
    pub kind: BlackboardProvenanceKind,
    /// Stable turn, maintenance operation, or import identity that produced this revision.
    pub source_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewBlackboardEntry {
    pub project_id: String,
    pub node_id: HierarchyNodeId,
    pub kind: BlackboardKind,
    pub content: String,
    pub structured_value: Option<BlackboardStructuredValue>,
    pub confidence: ConfidenceScore,
    pub verification: BlackboardVerification,
    pub importance: BlackboardImportance,
    pub root_promotion: RootPromotion,
    pub evidence: Vec<BlackboardEvidenceLink>,
    pub provenance: BlackboardProvenance,
}

impl NewBlackboardEntry {
    pub fn validate(&self) -> Result<(), BlackboardError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| BlackboardError::InvalidProjectId)?;
        validate_content(&self.content)?;
        validate_structured_value(self.structured_value.as_ref())?;
        validate_confidence(self.confidence)?;
        validate_evidence(self.verification, &self.evidence)?;
        validate_provenance(&self.provenance)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlackboardEntryUpdate {
    pub expected_revision: u64,
    pub kind: BlackboardKind,
    pub content: String,
    pub structured_value: Option<BlackboardStructuredValue>,
    pub confidence: ConfidenceScore,
    pub verification: BlackboardVerification,
    pub importance: BlackboardImportance,
    pub root_promotion: RootPromotion,
    pub evidence: Vec<BlackboardEvidenceLink>,
    pub state: BlackboardEntryState,
    pub superseded_by: Option<BlackboardEntryId>,
    pub provenance: BlackboardProvenance,
}

impl BlackboardEntryUpdate {
    pub fn validate(&self, entry_id: &BlackboardEntryId) -> Result<(), BlackboardError> {
        validate_content(&self.content)?;
        validate_structured_value(self.structured_value.as_ref())?;
        validate_confidence(self.confidence)?;
        validate_evidence(self.verification, &self.evidence)?;
        validate_provenance(&self.provenance)?;
        match (self.state, &self.superseded_by) {
            (BlackboardEntryState::Superseded, Some(successor)) if successor != entry_id => Ok(()),
            (BlackboardEntryState::Active | BlackboardEntryState::Tombstoned, None) => Ok(()),
            _ => Err(BlackboardError::InvalidSupersession),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlackboardEntry {
    pub id: BlackboardEntryId,
    pub value: NewBlackboardEntry,
    pub state: BlackboardEntryState,
    pub superseded_by: Option<BlackboardEntryId>,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

fn validate_content(value: &str) -> Result<(), BlackboardError> {
    if value.is_empty()
        || value.len() > MAX_CONTENT_BYTES
        || value.trim() != value
        || value.contains('\0')
    {
        return Err(BlackboardError::InvalidContent);
    }
    Ok(())
}

fn validate_structured_value(
    value: Option<&BlackboardStructuredValue>,
) -> Result<(), BlackboardError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.value.is_empty()
        || value.value.len() > MAX_STRUCTURED_VALUE_BYTES
        || value.value.trim() != value.value
        || value.value.contains('\0')
    {
        return Err(BlackboardError::InvalidStructuredValue);
    }
    if value.unit.as_ref().is_some_and(|unit| {
        unit.is_empty()
            || unit.len() > MAX_UNIT_BYTES
            || unit.trim() != unit
            || unit.chars().any(char::is_control)
    }) {
        return Err(BlackboardError::InvalidUnit);
    }
    Ok(())
}

fn validate_confidence(value: ConfidenceScore) -> Result<(), BlackboardError> {
    if value.0 > MAX_CONFIDENCE_BASIS_POINTS {
        return Err(BlackboardError::InvalidConfidence);
    }
    Ok(())
}

fn validate_evidence(
    verification: BlackboardVerification,
    evidence: &[BlackboardEvidenceLink],
) -> Result<(), BlackboardError> {
    if evidence.len() > MAX_EVIDENCE_LINKS {
        return Err(BlackboardError::TooManyEvidenceLinks);
    }
    let mut unique = HashSet::with_capacity(evidence.len());
    if evidence
        .iter()
        .any(|link| !unique.insert(link.context_map_entry_id.as_str()))
    {
        return Err(BlackboardError::DuplicateEvidenceLink);
    }
    if verification == BlackboardVerification::SourceVerified && evidence.is_empty() {
        return Err(BlackboardError::VerifiedWithoutEvidence);
    }
    Ok(())
}

fn validate_provenance(value: &BlackboardProvenance) -> Result<(), BlackboardError> {
    if value.source_id.is_empty()
        || value.source_id.len() > MAX_PROVENANCE_SOURCE_BYTES
        || value.source_id.trim() != value.source_id
        || value.source_id.chars().any(char::is_control)
    {
        return Err(BlackboardError::InvalidProvenance);
    }
    Ok(())
}

fn validate_identity(value: &str, maximum_bytes: usize) -> Result<(), ()> {
    if value.is_empty() || value.len() > maximum_bytes || value.chars().any(char::is_control) {
        return Err(());
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BlackboardError {
    #[error("blackboard entry ID must be non-empty, bounded, and contain no controls")]
    InvalidEntryId,
    #[error("project ID must be non-empty, bounded, and contain no controls")]
    InvalidProjectId,
    #[error("blackboard content must be non-empty, bounded, trimmed, and contain no NUL")]
    InvalidContent,
    #[error("structured value must be non-empty, bounded, trimmed, and contain no NUL")]
    InvalidStructuredValue,
    #[error("structured value unit must be non-empty, bounded, trimmed, and contain no controls")]
    InvalidUnit,
    #[error("confidence must be between 0 and 10000 basis points")]
    InvalidConfidence,
    #[error("blackboard entry has too many evidence links")]
    TooManyEvidenceLinks,
    #[error("blackboard evidence links must be unique by context-map entry")]
    DuplicateEvidenceLink,
    #[error("source-verified blackboard entries require evidence")]
    VerifiedWithoutEvidence,
    #[error("blackboard provenance must identify one bounded source without controls")]
    InvalidProvenance,
    #[error("superseded entries require a different successor; other states cannot name one")]
    InvalidSupersession,
}

#[cfg(test)]
#[path = "blackboard_tests.rs"]
mod tests;
