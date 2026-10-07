//! Project memory as the user reviews and corrects it, without a model turn.

use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

use super::BlackboardKind;
use super::BlackboardProvenanceKind;

/// Retained memory categories. These sections do not assert current applicability.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemorySection {
    /// The user's retained standing rule; scope eligibility is separate.
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

/// One active retained entry; its presence does not assert current applicability.
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
    /// On whose authority the entry rests, when recorded (older entries have none).
    pub authority: Option<StatefulMemoryAuthority>,
    /// The investigation a rule is limited to, in the user's words.
    pub scope_title: Option<String>,
    /// Scope state observed for this thread; absent for project-wide entries.
    pub scope_state: Option<StatefulMemoryScopeState>,
    /// Whose words a relayed note keeps, as the user named them.
    pub attributed_to: Option<String>,
}

/// On whose authority a memory entry rests.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryAuthority {
    /// The user's own words or control action.
    HumanDirect,
    /// What the assistant said; not the user's word and not verified.
    AssistantReported,
    /// Someone else's words the user passed on.
    ReportedThirdParty,
    /// Observed by the host (a command, a commit, source bytes).
    HostObserved,
    LegacyUnknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryReadParams {
    pub thread_id: String,
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
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
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
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
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
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

/// What the user adds to project memory.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryAddKind {
    /// A rule in the user's words, applied to new work within its scope.
    Rule,
    /// Something about the user or the whole work.
    Background,
    /// A decision, with its reason when given.
    Decision,
    /// Anything else worth keeping.
    Note,
}

/// Adds an entry in the user's own words, with no model turn.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryAddParams {
    pub thread_id: String,
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
    pub kind: StatefulMemoryAddKind,
    pub content: String,
    /// For a rule: where it applies, in the user's words ("this whole investigation, until
    /// we agree on the root cause"); absent means the whole project.
    #[ts(optional = nullable)]
    pub scope: Option<String>,
    /// For a decision: why it was made.
    #[ts(optional = nullable)]
    pub reason: Option<String>,
    /// Identifies this user action; replay returns its entry only while it remains active.
    /// A retired or superseded entry refuses replay without restoring anything.
    pub client_action_id: String,
    /// Set by clients that know the `background` section.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub background_section: bool,
}

/// What an addition did.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryAddOutcome {
    /// A new entry was stored, or kept words now apply.
    Added,
    /// The same words were already current; nothing changed.
    AlreadyPresent,
    /// This action was already carried out; its still-active entry is returned unchanged.
    AlreadyDone,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryAddResponse {
    pub item: StatefulMemoryItem,
    pub outcome: StatefulMemoryAddOutcome,
}

/// What the user does with the investigations rules can be limited to.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryScopeAction {
    /// List the project's investigations and which one this thread continues.
    List,
    /// This thread continues the named investigation; its rules apply here.
    Join,
    /// This thread no longer continues any investigation.
    Leave,
    /// End the named investigation; its rules stop applying everywhere (kept in history).
    End,
}

/// Lists, joins, leaves or ends an investigation for a thread, with no model turn.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryScopeParams {
    pub thread_id: String,
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
    pub action: StatefulMemoryScopeAction,
    /// The investigation for join and end.
    #[ts(optional = nullable)]
    pub scope_id: Option<String>,
}

/// One investigation rules can be limited to.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryScope {
    pub scope_id: String,
    /// The user's words naming it.
    pub title: String,
    pub open: bool,
    /// When its rules stop applying, in the user's words.
    pub end_condition: Option<String>,
    /// Whether this thread continues it.
    pub this_thread: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryScopeResponse {
    /// The project's investigations after the action, newest first.
    pub scopes: Vec<StatefulMemoryScope>,
}

/// The retained investigation scope as observed during review, without asserting application.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryScopeState {
    Open,
    NotBoundHere,
    Ended,
    Unknown,
}
