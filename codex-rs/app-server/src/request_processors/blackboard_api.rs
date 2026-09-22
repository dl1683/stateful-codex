use codex_app_server_protocol::BlackboardEntry as ApiEntry;
use codex_app_server_protocol::BlackboardEntryState as ApiEntryState;
use codex_app_server_protocol::BlackboardEvidenceFreshness as ApiEvidenceFreshness;
use codex_app_server_protocol::BlackboardEvidenceLink as ApiEvidenceLink;
use codex_app_server_protocol::BlackboardImportance as ApiImportance;
use codex_app_server_protocol::BlackboardKind as ApiKind;
use codex_app_server_protocol::BlackboardProvenance as ApiProvenance;
use codex_app_server_protocol::BlackboardProvenanceKind as ApiProvenanceKind;
use codex_app_server_protocol::BlackboardQueryHit as ApiHit;
use codex_app_server_protocol::BlackboardRelation as ApiRelation;
use codex_app_server_protocol::BlackboardRelationKind as ApiRelationKind;
use codex_app_server_protocol::BlackboardRootPromotion as ApiRootPromotion;
use codex_app_server_protocol::BlackboardStructuredValue as ApiStructuredValue;
use codex_app_server_protocol::BlackboardVerification as ApiVerification;
use codex_app_server_protocol::EvidenceLineRange as ApiEvidenceLineRange;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardRelation;
use codex_project_intelligence::BlackboardRelationKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SourceFingerprint;

use crate::error_code::invalid_params;

pub(super) fn api_hit(hit: BlackboardHit) -> ApiHit {
    ApiHit {
        entry: api_entry(hit.entry),
        relations: hit.relations.into_iter().map(api_relation).collect(),
        evidence_freshness: api_evidence_freshness(hit.evidence_freshness),
        effective_verification: api_verification(hit.effective_verification),
    }
}

pub(super) fn api_entry(entry: BlackboardEntry) -> ApiEntry {
    ApiEntry {
        id: entry.id.to_string(),
        project_id: entry.value.project_id,
        node_id: entry.value.node_id.to_string(),
        kind: api_kind(entry.value.kind),
        content: entry.value.content,
        structured_value: entry
            .value
            .structured_value
            .map(|value| ApiStructuredValue {
                value: value.value,
                unit: value.unit,
            }),
        confidence_basis_points: entry.value.confidence.basis_points(),
        verification: api_verification(entry.value.verification),
        importance: api_importance(entry.value.importance),
        root_promotion: api_root_promotion(entry.value.root_promotion),
        evidence: entry
            .value
            .evidence
            .into_iter()
            .map(|link| ApiEvidenceLink {
                context_map_entry_id: link.context_map_entry_id.to_string(),
                source_fingerprint: link.source_fingerprint.to_string(),
                line_range: link.line_range.map(|range| ApiEvidenceLineRange {
                    start: range.start,
                    end: range.end,
                }),
            })
            .collect(),
        provenance: api_provenance(entry.value.provenance),
        state: api_entry_state(entry.state),
        superseded_by: entry.superseded_by.map(|id| id.to_string()),
        revision: entry.revision,
        created_at: entry.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: entry.updated_at_ms.div_euclid(/*rhs*/ 1000),
    }
}

pub(super) fn api_relation(relation: BlackboardRelation) -> ApiRelation {
    ApiRelation {
        id: relation.id.to_string(),
        project_id: relation.value.project_id,
        from_entry_id: relation.value.from_entry_id.to_string(),
        to_entry_id: relation.value.to_entry_id.to_string(),
        kind: match relation.value.kind {
            BlackboardRelationKind::Supports => ApiRelationKind::Supports,
            BlackboardRelationKind::Contradicts => ApiRelationKind::Contradicts,
            BlackboardRelationKind::DependsOn => ApiRelationKind::DependsOn,
            BlackboardRelationKind::RelatedTo => ApiRelationKind::RelatedTo,
        },
        note: relation.value.note,
        confidence_basis_points: relation.value.confidence.basis_points(),
        provenance: api_provenance(relation.value.provenance),
        revision: relation.revision,
        created_at: relation.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: relation.updated_at_ms.div_euclid(/*rhs*/ 1000),
    }
}

fn api_kind(value: BlackboardKind) -> ApiKind {
    match value {
        BlackboardKind::Instruction => ApiKind::Instruction,
        BlackboardKind::Fact => ApiKind::Fact,
        BlackboardKind::Claim => ApiKind::Claim,
        BlackboardKind::Number => ApiKind::Number,
        BlackboardKind::Decision => ApiKind::Decision,
        BlackboardKind::Strategy => ApiKind::Strategy,
        BlackboardKind::Question => ApiKind::Question,
        BlackboardKind::Contradiction => ApiKind::Contradiction,
        BlackboardKind::Failure => ApiKind::Failure,
        BlackboardKind::RejectedApproach => ApiKind::RejectedApproach,
        BlackboardKind::Signal => ApiKind::Signal,
        BlackboardKind::Note => ApiKind::Note,
    }
}

fn api_verification(value: BlackboardVerification) -> ApiVerification {
    match value {
        BlackboardVerification::Unverified => ApiVerification::Unverified,
        BlackboardVerification::SourceVerified => ApiVerification::SourceVerified,
        BlackboardVerification::UserConfirmed => ApiVerification::UserConfirmed,
        BlackboardVerification::Disputed => ApiVerification::Disputed,
        BlackboardVerification::Stale => ApiVerification::Stale,
    }
}

fn api_importance(value: BlackboardImportance) -> ApiImportance {
    match value {
        BlackboardImportance::Critical => ApiImportance::Critical,
        BlackboardImportance::High => ApiImportance::High,
        BlackboardImportance::Normal => ApiImportance::Normal,
        BlackboardImportance::Low => ApiImportance::Low,
    }
}

fn api_root_promotion(value: RootPromotion) -> ApiRootPromotion {
    match value {
        RootPromotion::NotPromoted => ApiRootPromotion::NotPromoted,
        RootPromotion::Candidate => ApiRootPromotion::Candidate,
        RootPromotion::Promoted => ApiRootPromotion::Promoted,
    }
}

fn api_entry_state(value: BlackboardEntryState) -> ApiEntryState {
    match value {
        BlackboardEntryState::Active => ApiEntryState::Active,
        BlackboardEntryState::Superseded => ApiEntryState::Superseded,
        BlackboardEntryState::Tombstoned => ApiEntryState::Tombstoned,
    }
}

fn api_provenance(value: BlackboardProvenance) -> ApiProvenance {
    ApiProvenance {
        kind: match value.kind {
            BlackboardProvenanceKind::User => ApiProvenanceKind::User,
            BlackboardProvenanceKind::Agent => ApiProvenanceKind::Agent,
            BlackboardProvenanceKind::Maintenance => ApiProvenanceKind::Maintenance,
            BlackboardProvenanceKind::Import => ApiProvenanceKind::Import,
        },
        source_id: value.source_id,
    }
}

fn api_evidence_freshness(value: BlackboardEvidenceFreshness) -> ApiEvidenceFreshness {
    match value {
        BlackboardEvidenceFreshness::NotApplicable => ApiEvidenceFreshness::NotApplicable,
        BlackboardEvidenceFreshness::Current => ApiEvidenceFreshness::Current,
        BlackboardEvidenceFreshness::Stale => ApiEvidenceFreshness::Stale,
        BlackboardEvidenceFreshness::SourceUnavailable => ApiEvidenceFreshness::SourceUnavailable,
    }
}

pub(super) fn internal_evidence(
    value: Vec<ApiEvidenceLink>,
) -> Result<Vec<BlackboardEvidenceLink>, JSONRPCErrorError> {
    value
        .into_iter()
        .map(|link| {
            Ok(BlackboardEvidenceLink {
                context_map_entry_id: ContextMapEntryId::parse(link.context_map_entry_id)
                    .map_err(|error| invalid_params(error.to_string()))?,
                source_fingerprint: SourceFingerprint::parse(link.source_fingerprint)
                    .map_err(|error| invalid_params(error.to_string()))?,
                line_range: link.line_range.map(|range| EvidenceLineRange {
                    start: range.start,
                    end: range.end,
                }),
            })
        })
        .collect()
}

pub(super) fn internal_provenance(value: ApiProvenance) -> BlackboardProvenance {
    BlackboardProvenance {
        kind: match value.kind {
            ApiProvenanceKind::User => BlackboardProvenanceKind::User,
            ApiProvenanceKind::Agent => BlackboardProvenanceKind::Agent,
            ApiProvenanceKind::Maintenance => BlackboardProvenanceKind::Maintenance,
            ApiProvenanceKind::Import => BlackboardProvenanceKind::Import,
        },
        source_id: value.source_id,
    }
}

pub(super) fn internal_kind(value: ApiKind) -> BlackboardKind {
    match value {
        ApiKind::Instruction => BlackboardKind::Instruction,
        ApiKind::Fact => BlackboardKind::Fact,
        ApiKind::Claim => BlackboardKind::Claim,
        ApiKind::Number => BlackboardKind::Number,
        ApiKind::Decision => BlackboardKind::Decision,
        ApiKind::Strategy => BlackboardKind::Strategy,
        ApiKind::Question => BlackboardKind::Question,
        ApiKind::Contradiction => BlackboardKind::Contradiction,
        ApiKind::Failure => BlackboardKind::Failure,
        ApiKind::RejectedApproach => BlackboardKind::RejectedApproach,
        ApiKind::Signal => BlackboardKind::Signal,
        ApiKind::Note => BlackboardKind::Note,
    }
}

pub(super) fn internal_verification(value: ApiVerification) -> BlackboardVerification {
    match value {
        ApiVerification::Unverified => BlackboardVerification::Unverified,
        ApiVerification::SourceVerified => BlackboardVerification::SourceVerified,
        ApiVerification::UserConfirmed => BlackboardVerification::UserConfirmed,
        ApiVerification::Disputed => BlackboardVerification::Disputed,
        ApiVerification::Stale => BlackboardVerification::Stale,
    }
}

pub(super) fn internal_importance(value: ApiImportance) -> BlackboardImportance {
    match value {
        ApiImportance::Critical => BlackboardImportance::Critical,
        ApiImportance::High => BlackboardImportance::High,
        ApiImportance::Normal => BlackboardImportance::Normal,
        ApiImportance::Low => BlackboardImportance::Low,
    }
}

pub(super) fn internal_root_promotion(value: ApiRootPromotion) -> RootPromotion {
    match value {
        ApiRootPromotion::NotPromoted => RootPromotion::NotPromoted,
        ApiRootPromotion::Candidate => RootPromotion::Candidate,
        ApiRootPromotion::Promoted => RootPromotion::Promoted,
    }
}

pub(super) fn internal_state(value: ApiEntryState) -> BlackboardEntryState {
    match value {
        ApiEntryState::Active => BlackboardEntryState::Active,
        ApiEntryState::Superseded => BlackboardEntryState::Superseded,
        ApiEntryState::Tombstoned => BlackboardEntryState::Tombstoned,
    }
}

pub(super) fn internal_relation_kind(value: ApiRelationKind) -> BlackboardRelationKind {
    match value {
        ApiRelationKind::Supports => BlackboardRelationKind::Supports,
        ApiRelationKind::Contradicts => BlackboardRelationKind::Contradicts,
        ApiRelationKind::DependsOn => BlackboardRelationKind::DependsOn,
        ApiRelationKind::RelatedTo => BlackboardRelationKind::RelatedTo,
    }
}
