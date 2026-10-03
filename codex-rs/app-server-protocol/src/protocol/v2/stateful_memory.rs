//! Project memory as the user reviews and corrects it, without a model turn.

use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

use super::BlackboardKind;
use super::BlackboardProvenanceKind;

/// Where a memory item belongs, matching what new work applies.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemorySection {
    /// The user's standing rule in their own words; applied to new work.
    UserRule,
    /// The user's rule limited to a task; kept, never applied.
    PendingRule,
    /// A rule not in the user's own words; never applied.
    UnverifiedRule,
    Decision,
    /// What the user said about themselves or the whole work, in their own words. Sent only
    /// to clients that ask for it with `backgroundSection`.
    Background,
    Knowledge,
}

/// An earlier entry that a memory item replaced.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryReplaced {
    pub entry_id: String,
    /// At most 240 bytes.
    pub content: String,
    /// Unix seconds.
    pub replaced_at: i64,
}

/// One current entry of project memory.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryItem {
    pub entry_id: String,
    #[ts(type = "number")]
    pub revision: u64,
    pub section: StatefulMemorySection,
    pub kind: BlackboardKind,
    /// At most 2,000 bytes; `contentTruncated` says whether more exists.
    pub content: String,
    pub content_truncated: bool,
    /// Who wrote this text.
    pub source: BlackboardProvenanceKind,
    /// Unix seconds.
    pub updated_at: i64,
    /// What this entry replaced, newest first, at most three.
    pub replaces: Vec<StatefulMemoryReplaced>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryReadParams {
    pub thread_id: String,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    /// At most 100; defaults to 50.
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
    /// Set by clients that know the `background` section; without it, the user's background
    /// is reported in the `knowledge` section so older clients keep working.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub background_section: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryReadResponse {
    pub project_id: String,
    /// User rules first, in the order the user stated them, then other rules, decisions and
    /// other knowledge, most recently changed first within each.
    pub data: Vec<StatefulMemoryItem>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryForgetParams {
    pub thread_id: String,
    pub entry_id: String,
    #[ts(type = "number")]
    pub expected_revision: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryForgetResponse {
    pub entry_id: String,
    #[ts(type = "number")]
    pub revision: u64,
}

/// Replaces an entry with the user's corrected words. Correcting a rule not in the user's
/// words makes the correction the user's standing rule.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryCorrectParams {
    pub thread_id: String,
    pub entry_id: String,
    #[ts(type = "number")]
    pub expected_revision: u64,
    pub content: String,
    /// Set by clients that know the `background` section; without it, the user's background
    /// is reported in the `knowledge` section so older clients keep working.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub background_section: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryCorrectResponse {
    pub item: StatefulMemoryItem,
}
