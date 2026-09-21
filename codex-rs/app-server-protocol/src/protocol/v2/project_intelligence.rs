use crate::JsonSchema;
use crate::TS;
use codex_experimental_api_macros::ExperimentalApi;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;

use super::ContextMapSource;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectIntelligenceStatusParams {
    pub project_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectIntelligenceStatusResponse {
    pub project_id: String,
    pub roots: Vec<AbsolutePathBuf>,
    pub initialized: bool,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub hierarchy_node_count: u64,
    #[ts(type = "number")]
    pub file_count: u64,
    #[ts(type = "number")]
    pub missing_source_count: u64,
    #[ts(type = "number")]
    pub context_map_entry_count: u64,
    #[ts(type = "number")]
    pub blackboard_entry_count: u64,
    #[ts(type = "number")]
    pub promoted_entry_count: u64,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct EvidenceReadParams {
    pub project_id: String,
    pub context_map_entry_id: String,
    #[ts(optional = nullable)]
    pub max_bytes: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct EvidenceReadResponse {
    pub project_id: String,
    pub context_map_entry_id: String,
    pub node_id: String,
    pub source_fingerprint: String,
    pub source: ContextMapSource,
    pub encoding: EvidenceEncoding,
    pub content: String,
    #[ts(type = "number")]
    pub bytes_returned: u64,
    #[ts(type = "number")]
    pub total_bytes: u64,
    pub truncated: bool,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum EvidenceEncoding {
    Utf8,
    Base64,
}
