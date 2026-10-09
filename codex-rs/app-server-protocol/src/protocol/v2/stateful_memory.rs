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
    /// Historical scope disposition; unsupported scopes are held back in every thread.
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
    /// An unscoped rule in the user's words, applied to new project work.
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
    /// Legacy field: any supplied rule scope is refused. Omit for an unscoped addition.
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

/// Legacy actions decoded solely to return an explicit unsupported error.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryScopeAction {
    /// Former list action; unsupported.
    List,
    /// Former join action; unsupported.
    Join,
    /// Former leave action; unsupported.
    Leave,
    /// Former end action; unsupported.
    End,
}

/// Legacy scope request. Every action is refused; no collection is returned.
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

/// Legacy RPC response marker needed by the request registry. This method always returns
/// an unsupported error; there is no successful collection response.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryScopeResponse {}

/// The retained investigation scope as observed during review, without asserting application.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryScopeState {
    /// Historical scoped entry: application is unsupported and always held back.
    Unsupported,
    Open,
    NotBoundHere,
    Ended,
    Unknown,
}

/// What a memory receipt reports.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryReceiptKind {
    /// Standing project rules the user stated in a supported declaration.
    Rules,
    /// Material kept for review; nothing was applied.
    Proposals,
    /// A proposal the user applied.
    Promotion,
    /// A receipt the user undid.
    Undo,
}

/// What became of one unit of a receipt.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryReceiptStatus {
    Saved,
    AlreadyPresent,
    /// Kept for review, not applied.
    Proposed,
    /// Kept, not applied, with an unresolved dependency.
    Pending,
    /// Recognized but not stored whole; the exact source remains.
    Omitted,
    /// Words the user forgot earlier; a later statement does not restore them.
    NotRestored,
    /// Held back with the rest of a refused declaration.
    Refused,
    /// Retired by an Undo.
    Undone,
    /// An Undo returned an applied proposal to review.
    ProposalRestored,
    /// Existed before the undone receipt; left as it was.
    Untouched,
}

/// The kind of knowledge a receipt member holds.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryReceiptCategory {
    Rule,
    Background,
    AttributedContext,
    Decision,
    BrainstormOption,
    RuledOut,
    OpenCheck,
    Note,
    Other,
}

/// One unit of a memory receipt.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryReceiptMember {
    /// Absent when nothing was stored for this unit.
    pub entry_id: Option<String>,
    #[ts(type = "number | null")]
    pub revision: Option<u64>,
    pub category: StatefulMemoryReceiptCategory,
    pub status: StatefulMemoryReceiptStatus,
    /// At most 240 bytes. For proposals this is the assistant's labelled reading, not the
    /// user's words; applied and stated entries show the user's exact words.
    pub text: String,
    /// Whether `text` is shorter than the stored words; the exact words stay readable under
    /// `entryId`@`revision` in `statefulMemory/read`.
    pub text_shortened: bool,
    /// For a kept proposal that can be applied: the user's exact words an Apply would make
    /// theirs, at most 240 bytes, separate from the assistant's reading in `text`. Absent when
    /// an Apply would refuse (partial citation, unresolved scope or other dependency).
    pub applies_text: Option<String>,
    pub applies_text_shortened: bool,
}

/// A committed (or refused) memory capture, Apply or Undo. Counts are by member status:
/// saved rules are not proposals, and a refusal never reports a save.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryReceipt {
    pub project_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    /// Pass to `statefulMemory/undo`.
    pub receipt_id: String,
    pub kind: StatefulMemoryReceiptKind,
    /// At most 24.
    pub members: Vec<StatefulMemoryReceiptMember>,
    /// Whether one Undo can reverse what this receipt saved or proposed.
    pub undoable: bool,
}

/// Sent when a turn's words were captured, or a capture was refused, or the user applied a
/// proposal or undid a receipt.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryCapturedNotification {
    pub receipt: StatefulMemoryReceipt,
}

/// What the user applies a proposal as. Applied words take project scope.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulMemoryApplyCategory {
    /// A standing project rule in the user's quoted words.
    Rule,
    /// The user's settled decision with its quoted reason.
    Decision,
    /// An approach the user ruled out, with its quoted reason.
    RuledOut,
}

/// Applies a kept proposal as the user's own words, with no model turn.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryApplyParams {
    pub thread_id: String,
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
    pub entry_id: String,
    #[ts(type = "number")]
    pub expected_revision: u64,
    pub category: StatefulMemoryApplyCategory,
    /// Identifies this user action; a retry returns the recorded receipt.
    pub client_action_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryApplyResponse {
    pub receipt: StatefulMemoryReceipt,
}

/// Undoes one receipt with no model turn: what it newly saved is retired, an applied
/// proposal returns to review, and anything changed since conflicts without partial undo.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryUndoParams {
    pub thread_id: String,
    /// The project observed by the client at submission; mismatch refuses the operation.
    pub expected_project_id: String,
    pub receipt_id: String,
    /// Identifies this user action; a retry returns the recorded result.
    pub client_action_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMemoryUndoResponse {
    pub receipt: StatefulMemoryReceipt,
}
