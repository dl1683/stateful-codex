//! What project memory holds and what changed in it, counted from the journal of committed
//! memory changes: the passive status line, session and exit receipts, the activity list, and
//! the dated return recap. Nothing here is estimated from client-side notifications.

use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

/// What a committed memory change did.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryOperation {
    Saved,
    /// Kept words started to apply.
    Promoted,
    Corrected,
    Forgotten,
    /// No longer current (the source it described changed).
    Invalidated,
    /// An investigation ended, so its rules stopped applying.
    ScopeEnded,
    /// A capture could not finish; something may be missing.
    CaptureIncomplete,
}

/// Who caused a memory change.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryOrigin {
    /// Captured from the user's own words.
    HostCapture,
    /// The user's own memory control (TUI, web panel, CLI).
    DirectControl,
    /// Written by the model.
    ModelTool,
    /// Observed in the workspace (a commit in its history).
    HostObserved,
}

/// What kind of knowledge a change concerned.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryChangeCategory {
    Rule,
    Background,
    AttributedContext,
    Decision,
    BrainstormOption,
    RuledOut,
    OpenCheck,
    Recipe,
    CodeObservation,
    CommitObservation,
    Note,
    Legacy,
}

/// One committed memory change.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryChange {
    /// Position in the project's journal; strictly increasing.
    #[ts(type = "number")]
    pub sequence: u64,
    pub entry_id: Option<String>,
    #[ts(type = "number | null")]
    pub revision: Option<u64>,
    pub operation: StatefulMemoryOperation,
    pub origin: StatefulMemoryOrigin,
    pub category: StatefulMemoryChangeCategory,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub group_id: Option<String>,
    /// At most 240 bytes, cut on a character boundary.
    pub preview: String,
    /// Unix seconds.
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryActivityParams {
    /// A thread of the project whose journal is read.
    pub thread_id: String,
    /// Continue after a page; omit to start after `afterSequence` (or the beginning).
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    /// Only changes after this journal sequence.
    #[ts(optional = nullable, type = "number | null")]
    pub after_sequence: Option<u64>,
    /// Only changes recorded for these threads.
    #[ts(optional = nullable)]
    pub thread_ids: Option<Vec<String>>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryActivityResponse {
    pub project_id: String,
    /// Oldest first.
    pub data: Vec<StatefulMemoryChange>,
    pub next_cursor: Option<String>,
    /// The newest journal sequence of the project when this page was read.
    #[ts(type = "number")]
    pub latest_sequence: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemorySummaryParams {
    pub thread_id: String,
    /// Also count the changes after this journal sequence (a session's start watermark).
    /// Counted by thread, not by client: another client acting in the same threads after
    /// the watermark is counted too.
    #[ts(optional = nullable, type = "number | null")]
    pub since_sequence: Option<u64>,
    /// Restrict those counted changes to these threads (a session's threads).
    #[ts(optional = nullable)]
    pub thread_ids: Option<Vec<String>>,
}

/// What current project memory holds, by what new work does with it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryCounts {
    /// The user's standing rules, applied.
    pub rules: u32,
    /// The user's task-limited rules, kept but not applied.
    pub pending_rules: u32,
    /// Rules not in the user's words, never applied.
    pub unverified_rules: u32,
    pub background: u32,
    pub decisions: u32,
    pub open_checks: u32,
    /// Commits remembered from the workspace history.
    pub commits: u32,
    pub other: u32,
}

/// Committed changes counted over a stretch of the journal.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryChangeTotals {
    /// New entries saved, commits remembered not included.
    pub saved: u32,
    /// Commits remembered from the workspace history.
    pub commits_remembered: u32,
    pub promoted: u32,
    pub corrected: u32,
    pub forgotten: u32,
    pub invalidated: u32,
    pub scopes_ended: u32,
    /// Captures that could not finish.
    pub capture_incomplete: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemorySummaryResponse {
    pub project_id: String,
    pub counts: StatefulMemoryCounts,
    /// The newest journal sequence when this summary was read; a session starts here.
    #[ts(type = "number")]
    pub latest_sequence: u64,
    /// The changes after `sinceSequence`, when it was given.
    pub since: Option<StatefulMemoryChangeTotals>,
    /// Unix seconds of the newest journaled change, if any.
    #[ts(type = "number | null")]
    pub last_change_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryRecapParams {
    /// The thread being returned to; its project's memory and its investigation apply.
    pub thread_id: String,
}

/// The last finished piece of work in the project.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRecapWork {
    pub thread_id: String,
    /// Unix seconds when it finished.
    #[ts(type = "number")]
    pub finished_at: i64,
    /// The opening of what was asked, at most 240 bytes.
    pub request: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRecapDecision {
    /// At most 240 bytes.
    pub text: String,
    /// The recorded reason, when one was recorded; at most 240 bytes.
    pub reason: Option<String>,
}

/// A dated return card assembled from stored memory, with no model call. Every list is
/// bounded; the `more*` counts say how much was left out.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryRecapResponse {
    pub project_id: String,
    /// Unix seconds this recap was assembled.
    #[ts(type = "number")]
    pub as_of: i64,
    pub last_work: Option<StatefulRecapWork>,
    /// Rules that apply to this thread, in the order written; at most 240 bytes each.
    pub rules: Vec<String>,
    pub more_rules: u32,
    /// Current decisions, newest first.
    pub decisions: Vec<StatefulRecapDecision>,
    pub more_decisions: u32,
    pub open_checks: Vec<String>,
    pub more_open_checks: u32,
    /// Commits from the workspace history still remembered, most recently remembered first.
    pub commits: Vec<String>,
    pub more_commits: u32,
    /// Captures that could not finish since the last finished work.
    pub capture_incomplete: u32,
}
