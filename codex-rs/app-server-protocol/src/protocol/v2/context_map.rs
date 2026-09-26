use crate::JsonSchema;
use crate::TS;
use codex_experimental_api_macros::ExperimentalApi;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapQueryParams {
    pub project_id: String,
    pub text: String,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapQueryResponse {
    pub data: Vec<ContextMapQueryHit>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapRefreshParams {
    pub project_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapRefreshResponse {
    #[ts(type = "number")]
    pub files_indexed: u64,
    #[ts(type = "number")]
    pub regions_indexed: u64,
    #[ts(type = "number")]
    pub files_skipped: u64,
    #[ts(type = "number")]
    pub missing_files: u64,
    pub truncated: bool,
    #[ts(type = "number")]
    pub scan_duration_ms: u64,
    #[ts(type = "number")]
    pub publication_duration_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapQueryHit {
    pub entry_id: String,
    pub node_id: String,
    pub source_fingerprint: String,
    pub description: String,
    pub routing_terms: Vec<String>,
    pub coverage: ContextMapCoverage,
    pub freshness: ContextMapFreshness,
    pub source: ContextMapSource,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
    #[ts(type = "number | null")]
    pub last_verified_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum ContextMapCoverage {
    Complete,
    Partial,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum ContextMapFreshness {
    Current,
    Stale,
    SourceUnavailable,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapSource {
    pub project_root: AbsolutePathBuf,
    pub relative_path: String,
    pub region_anchor: Option<ContextMapRegionAnchor>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ContextMapRegionAnchor {
    pub scheme: String,
    pub locator: String,
}
