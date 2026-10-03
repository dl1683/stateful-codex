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
}

/// What became of one unit a capture recognized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberOutcome {
    Saved,
    AlreadyPresent,
    /// The same words were saved once and later forgotten or replaced; a new source does not
    /// bring them back.
    NotRestored,
    /// Recognized but not kept (too long to keep whole, or not a valid entry).
    Omitted,
}

/// One unit of a capture, in source order.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureUnit {
    /// A new entry under `id`, unless that identity is already stored, or it or one of
    /// `retired_identities` (the same words elsewhere) was forgotten or replaced.
    Entry {
        id: crate::BlackboardEntryId,
        retired_identities: Vec<crate::BlackboardEntryId>,
        value: Box<crate::NewBlackboardEntry>,
        context: KnowledgeContext,
        change: ChangeRecord,
    },
    /// An entry already holding exactly these words (a model write of this turn); it gets
    /// `context` if it has none and is listed as already present.
    Existing {
        id: crate::BlackboardEntryId,
        context: KnowledgeContext,
    },
    /// Recognized but not kept; `note` says what it was and why.
    Omitted { note: String },
}

/// Where a capture's units were read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureSource {
    /// Stable route to the source ("assistant-answer:<thread>/<turn>/<item>").
    pub locator: String,
    /// SHA-256 of the exact source text, hex.
    pub digest: String,
}

/// One member of a stored capture group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureMember {
    pub ordinal: u32,
    pub entry_id: Option<String>,
    pub outcome: MemberOutcome,
    pub note: Option<String>,
}

/// A committed capture: the group with its counts, where it was read from, and its members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedCapture {
    pub group: CaptureGroup,
    pub source: Option<CaptureSource>,
    pub members: Vec<CaptureMember>,
    /// False when the same source was committed before and this returned that result.
    pub newly_committed: bool,
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
    NotRestored => "not_restored",
    Omitted => "omitted",
});
