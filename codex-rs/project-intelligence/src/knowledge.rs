//! Revision-bound meaning of a blackboard entry beyond its kind: what category of knowledge it
//! is, on whose authority it rests, where it applies, where it came from in the conversation,
//! and whether it is still current. Plus the investigation scopes rules can be bound to, the
//! groups one capture produced, and the journal of committed memory changes that receipts and
//! summaries are counted from.

use std::str::FromStr;

/// What kind of knowledge an entry holds, independent of its blackboard kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnowledgeCategory {
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

/// On whose authority an entry rests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnowledgeAuthority {
    /// The user's own words or direct control action.
    HumanDirect,
    /// What the assistant said; not the user's word and not verified by the host.
    AssistantReported,
    /// Someone else's words the user passed on.
    ReportedThirdParty,
    /// Observed by the host (a command, a commit, source bytes).
    HostObserved,
    LegacyUnknown,
}

/// Whether an entry still describes the present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnowledgeValidity {
    Current,
    NeedsCheck,
    Obsolete,
    Historical,
}

/// The meaning recorded with one entry revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnowledgeContext {
    pub category: KnowledgeCategory,
    pub authority: KnowledgeAuthority,
    /// The investigation or task scope it applies in; `None` means the whole project.
    pub scope_id: Option<String>,
    /// The user's words for when it stops applying ("until we agree on the root cause").
    pub end_condition: Option<String>,
    /// Position of the source message in the project's capture order.
    pub source_sequence: Option<u64>,
    /// Position of the unit within its source message.
    pub unit_ordinal: Option<u32>,
    pub group_id: Option<String>,
    pub validity: KnowledgeValidity,
    /// Small typed details as JSON (a decision's reason, an observation's anchors).
    pub payload: Option<String>,
}

impl KnowledgeContext {
    /// A context for `category` on `authority`, applying project-wide and current.
    pub fn new(category: KnowledgeCategory, authority: KnowledgeAuthority) -> Self {
        Self {
            category,
            authority,
            scope_id: None,
            end_condition: None,
            source_sequence: None,
            unit_ordinal: None,
            group_id: None,
            validity: KnowledgeValidity::Current,
            payload: None,
        }
    }
}

/// What a scope covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    Investigation,
    Task,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeState {
    Open,
    Ended,
}

/// An investigation or task that scoped rules apply in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnowledgeScope {
    pub project_id: String,
    pub scope_id: String,
    pub kind: ScopeKind,
    /// The user's words naming it ("Some ground rules for this whole investigation").
    pub title: String,
    pub state: ScopeState,
    pub end_condition: Option<String>,
    pub opened_source: String,
    pub ended_source: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// A committed change to project memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOperation {
    Saved,
    /// Kept words started to apply (a task-limited rule restated as standing).
    Promoted,
    Corrected,
    Forgotten,
    Invalidated,
    ScopeEnded,
    CaptureIncomplete,
}

/// Who caused a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOrigin {
    /// The host captured the user's words at turn start or from a completed answer.
    HostCapture,
    /// The user's own control (/memory, the web panel, the CLI).
    DirectControl,
    /// A model tool call.
    ModelTool,
    /// The host observed the workspace (a commit, changed source).
    HostObserved,
}

/// What a writer records about the change it commits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRecord {
    pub operation: ChangeOperation,
    pub origin: ChangeOrigin,
    pub category: KnowledgeCategory,
    pub action_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub group_id: Option<String>,
    /// At most `MAX_CHANGE_PREVIEW_BYTES`, cut on a character boundary.
    pub preview: String,
}

/// Longest preview kept with a change.
pub const MAX_CHANGE_PREVIEW_BYTES: usize = 240;

/// One journal row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryChange {
    pub project_id: String,
    pub sequence: u64,
    pub entry_id: Option<String>,
    pub revision: Option<u64>,
    pub record: ChangeRecord,
    pub created_at_ms: i64,
}

/// What became of one recognized unit of a capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberOutcome {
    Saved,
    AlreadyPresent,
    /// Saved but not applied (limited to a task).
    Pending,
    /// Recognized but not stored (too long to keep whole).
    Omitted,
    Failed,
    /// An earlier source cannot restore wording retired by the user.
    NotRestored,
}

/// One unit of a capture, in the order the user wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureGroupMember {
    pub ordinal: u32,
    pub entry_id: Option<String>,
    pub revision: Option<u64>,
    pub outcome: MemberOutcome,
    /// At most `MAX_CHANGE_PREVIEW_BYTES`.
    pub preview: String,
    pub reason: Option<String>,
}

/// The outcome of one capture: what it recognized and what became of each unit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureGroup {
    pub project_id: String,
    pub group_id: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    /// What the group holds ("rules", "background", "ruledOut").
    pub kind: String,
    /// A count the user's words declared ("Two standing rules"), when they did.
    pub declared_count: Option<u32>,
    pub recognized: u32,
    pub saved: u32,
    pub already_present: u32,
    pub pending: u32,
    pub omitted: u32,
    pub failed: u32,
    /// Every recognized unit and what became of it, in the order written.
    pub members: Vec<CaptureGroupMember>,
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    other => Err(format!("unknown {}: {other}", stringify!($name))),
                }
            }
        }
    };
}

string_enum!(KnowledgeCategory {
    Rule => "rule",
    Background => "background",
    AttributedContext => "attributed_context",
    Decision => "decision",
    BrainstormOption => "brainstorm_option",
    RuledOut => "ruled_out",
    OpenCheck => "open_check",
    Recipe => "recipe",
    CodeObservation => "code_observation",
    CommitObservation => "commit_observation",
    Note => "note",
    Legacy => "legacy",
});
string_enum!(KnowledgeAuthority {
    HumanDirect => "human_direct",
    AssistantReported => "assistant_reported",
    ReportedThirdParty => "reported_third_party",
    HostObserved => "host_observed",
    LegacyUnknown => "legacy_unknown",
});
string_enum!(KnowledgeValidity {
    Current => "current",
    NeedsCheck => "needs_check",
    Obsolete => "obsolete",
    Historical => "historical",
});
string_enum!(ScopeKind {
    Investigation => "investigation",
    Task => "task",
});
string_enum!(ScopeState {
    Open => "open",
    Ended => "ended",
});
string_enum!(ChangeOperation {
    Saved => "saved",
    Promoted => "promoted",
    Corrected => "corrected",
    Forgotten => "forgotten",
    Invalidated => "invalidated",
    ScopeEnded => "scope_ended",
    CaptureIncomplete => "capture_incomplete",
});
string_enum!(ChangeOrigin {
    HostCapture => "host_capture",
    DirectControl => "direct_control",
    ModelTool => "model_tool",
    HostObserved => "host_observed",
});
string_enum!(MemberOutcome {
    Saved => "saved",
    AlreadyPresent => "already_present",
    Pending => "pending",
    Omitted => "omitted",
    Failed => "failed",
    NotRestored => "not_restored",
});

/// How investigations bear on one thread, read with the root projection it explains.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadScopes {
    /// The scope the thread is bound to, open or ended.
    pub bound: Option<KnowledgeScope>,
    /// All of the project's scopes, newest first.
    pub scopes: Vec<KnowledgeScope>,
    /// Promoted entries left out because they belong to an open investigation the thread does
    /// not continue (entries of ended investigations are left out without a count).
    pub scoped_elsewhere: u64,
    /// Older rules naming a piece of work without a recorded scope, left out everywhere.
    pub legacy_held_back: u64,
}
