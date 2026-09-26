use crate::JsonSchema;
use crate::TS;
use codex_experimental_api_macros::ExperimentalApi;
use serde::Deserialize;
use serde::Serialize;

use super::EvidenceLineRange;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardQueryParams {
    pub project_id: String,
    #[ts(optional = nullable)]
    pub text: Option<String>,
    #[ts(optional = nullable)]
    pub within_node_id: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardQueryResponse {
    pub data: Vec<BlackboardQueryHit>,
    pub truncated: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardUpsertParams {
    pub project_id: String,
    pub entry_id: String,
    #[ts(optional = nullable)]
    pub expected_revision: Option<u64>,
    #[ts(optional = nullable)]
    pub node_id: Option<String>,
    pub kind: BlackboardKind,
    pub content: String,
    #[ts(optional = nullable)]
    pub structured_value: Option<BlackboardStructuredValue>,
    pub confidence_basis_points: u16,
    pub verification: BlackboardVerification,
    pub importance: BlackboardImportance,
    pub root_promotion: BlackboardRootPromotion,
    pub evidence: Vec<BlackboardEvidenceLink>,
    pub provenance: BlackboardProvenance,
    #[ts(optional = nullable)]
    pub state: Option<BlackboardEntryState>,
    #[ts(optional = nullable)]
    pub superseded_by: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardUpsertResponse {
    pub entry: BlackboardEntry,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardConfirmParams {
    pub project_id: String,
    pub entry_id: String,
    #[ts(type = "number")]
    pub expected_revision: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardConfirmResponse {
    pub entry: BlackboardEntry,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardRelateParams {
    pub project_id: String,
    pub relation_id: String,
    pub from_entry_id: String,
    pub to_entry_id: String,
    pub kind: BlackboardRelationKind,
    #[ts(optional = nullable)]
    pub note: Option<String>,
    pub confidence_basis_points: u16,
    pub provenance: BlackboardProvenance,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardRelateResponse {
    pub relation: BlackboardRelation,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardQueryHit {
    pub entry: BlackboardEntry,
    pub relations: Vec<BlackboardRelation>,
    pub evidence_freshness: BlackboardEvidenceFreshness,
    pub effective_verification: BlackboardVerification,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardEntry {
    pub id: String,
    pub project_id: String,
    pub node_id: String,
    pub kind: BlackboardKind,
    pub content: String,
    pub structured_value: Option<BlackboardStructuredValue>,
    pub confidence_basis_points: u16,
    pub verification: BlackboardVerification,
    pub importance: BlackboardImportance,
    pub root_promotion: BlackboardRootPromotion,
    pub evidence: Vec<BlackboardEvidenceLink>,
    pub provenance: BlackboardProvenance,
    pub state: BlackboardEntryState,
    pub superseded_by: Option<String>,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardRelation {
    pub id: String,
    pub project_id: String,
    pub from_entry_id: String,
    pub to_entry_id: String,
    pub kind: BlackboardRelationKind,
    pub note: Option<String>,
    pub confidence_basis_points: u16,
    pub provenance: BlackboardProvenance,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardStructuredValue {
    pub value: String,
    pub unit: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardEvidenceLink {
    pub context_map_entry_id: String,
    pub source_fingerprint: String,
    pub line_range: Option<EvidenceLineRange>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardProvenance {
    pub kind: BlackboardProvenanceKind,
    pub source_id: String,
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(rename_all = "camelCase", export_to = "v2/")]
        pub enum $name {
            $($variant),+
        }
    };
}

string_enum!(BlackboardKind {
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
});
string_enum!(BlackboardVerification {
    Unverified,
    SourceVerified,
    UserConfirmed,
    Disputed,
    Stale,
});
string_enum!(BlackboardImportance {
    Critical,
    High,
    Normal,
    Low
});
string_enum!(BlackboardRootPromotion {
    NotPromoted,
    Candidate,
    Promoted
});
string_enum!(BlackboardEntryState {
    Active,
    Superseded,
    Tombstoned
});
string_enum!(BlackboardProvenanceKind {
    User,
    Agent,
    Maintenance,
    Import
});
string_enum!(BlackboardRelationKind {
    Supports,
    Contradicts,
    DependsOn,
    RelatedTo
});
string_enum!(BlackboardEvidenceFreshness {
    NotApplicable,
    Current,
    Stale,
    SourceUnavailable,
});
string_enum!(BlackboardEntityKind { Entry, Relation });

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct BlackboardUpdatedNotification {
    pub project_id: String,
    pub entity_kind: BlackboardEntityKind,
    pub entity_id: String,
    #[ts(type = "number")]
    pub revision: u64,
    pub cursor: String,
}
