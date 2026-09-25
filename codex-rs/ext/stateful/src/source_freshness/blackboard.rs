use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardVerification;
use serde::Serialize;

use super::EvidenceAudit;
use super::SourceAuditStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AuditedEvidenceFreshness {
    NotApplicable,
    Current,
    Stale,
    SourceUnavailable,
    UncheckedThisTurn,
}

pub(crate) fn audited_blackboard_freshness(
    hit: &BlackboardHit,
    audit: Option<&EvidenceAudit>,
) -> AuditedEvidenceFreshness {
    let stored = match hit.evidence_freshness {
        BlackboardEvidenceFreshness::NotApplicable => AuditedEvidenceFreshness::NotApplicable,
        BlackboardEvidenceFreshness::Current => AuditedEvidenceFreshness::Current,
        BlackboardEvidenceFreshness::Stale => AuditedEvidenceFreshness::Stale,
        BlackboardEvidenceFreshness::SourceUnavailable => {
            AuditedEvidenceFreshness::SourceUnavailable
        }
    };
    if !matches!(stored, AuditedEvidenceFreshness::Current) {
        return stored;
    }
    let Some(audit) = audit else {
        return stored;
    };
    let mut result = AuditedEvidenceFreshness::Current;
    for evidence in &hit.entry.value.evidence {
        match audit.statuses.get(&evidence.context_map_entry_id) {
            Some(SourceAuditStatus::Current) => {}
            Some(SourceAuditStatus::Stale) => result = AuditedEvidenceFreshness::Stale,
            Some(SourceAuditStatus::SourceUnavailable) => {
                return AuditedEvidenceFreshness::SourceUnavailable;
            }
            Some(SourceAuditStatus::Unchecked) | None => {
                if matches!(result, AuditedEvidenceFreshness::Current) {
                    result = AuditedEvidenceFreshness::UncheckedThisTurn;
                }
            }
        }
    }
    result
}

pub(crate) fn audited_verification(
    declared: BlackboardVerification,
    freshness: AuditedEvidenceFreshness,
) -> BlackboardVerification {
    match (declared, freshness) {
        (BlackboardVerification::SourceVerified, AuditedEvidenceFreshness::Current) => {
            BlackboardVerification::SourceVerified
        }
        (BlackboardVerification::SourceVerified, AuditedEvidenceFreshness::UncheckedThisTurn) => {
            BlackboardVerification::Unverified
        }
        (BlackboardVerification::SourceVerified, _) => BlackboardVerification::Stale,
        (verification, _) => verification,
    }
}
