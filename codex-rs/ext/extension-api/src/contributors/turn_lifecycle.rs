//! Turn lifecycle inputs and scheduling phases for host-owned contributors.

use codex_protocol::config_types::CollaborationMode;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::user_input::UserInput;

use crate::ExtensionData;

/// Runs before task registration or during cancellable regular-task startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStartPhase {
    BeforeTaskRegistration,
    RegularTaskStart,
}

/// Input supplied when the host starts a turn.
pub struct TurnStartInput<'a> {
    /// Stable host-owned turn identifier.
    pub turn_id: &'a str,
    /// Effective collaboration mode for this turn.
    pub collaboration_mode: &'a CollaborationMode,
    /// Total token usage snapshot captured when the turn started.
    /// Present before task registration; absent during regular-task preparation.
    /// Token-accounting contributors must remain in `BeforeTaskRegistration`.
    pub token_usage_at_turn_start: Option<&'a TokenUsage>,
    /// User-origin input submitted to start this turn. Present before task registration;
    /// empty during regular-task preparation and for turns started without user input.
    pub user_input: &'a [UserInput],
    /// Store scoped to the host session runtime.
    pub session_store: &'a ExtensionData,
    /// Store scoped to this thread runtime.
    pub thread_store: &'a ExtensionData,
    /// Store scoped to this turn runtime.
    pub turn_store: &'a ExtensionData,
}

/// Input supplied when the host completes a turn.
pub struct TurnStopInput<'a> {
    /// Store scoped to the host session runtime.
    pub session_store: &'a ExtensionData,
    /// Store scoped to this thread runtime.
    pub thread_store: &'a ExtensionData,
    /// Store scoped to this turn runtime.
    pub turn_store: &'a ExtensionData,
}

/// Input supplied when the host aborts a turn.
pub struct TurnAbortInput<'a> {
    /// Reason the host aborted the turn.
    pub reason: TurnAbortReason,
    /// Store scoped to the host session runtime.
    pub session_store: &'a ExtensionData,
    /// Store scoped to this thread runtime.
    pub thread_store: &'a ExtensionData,
    /// Store scoped to this turn runtime.
    pub turn_store: &'a ExtensionData,
}

/// Input supplied when the host observes an error for a turn.
pub struct TurnErrorInput<'a> {
    /// Stable host-owned turn identifier.
    pub turn_id: &'a str,
    /// Error surfaced by the host for this turn.
    pub error: CodexErrorInfo,
    /// Original error details, including backend metadata omitted from the public category.
    pub error_details: &'a CodexErrorDetails,
    /// Store scoped to the host session runtime.
    pub session_store: &'a ExtensionData,
    /// Store scoped to this thread runtime.
    pub thread_store: &'a ExtensionData,
    /// Store scoped to this turn runtime.
    pub turn_store: &'a ExtensionData,
}

/// Input supplied when a regular task finalizes after its last model response.
///
/// The host calls this inside the task, while it is still registered and interruptible, only
/// when the turn ended normally with a nonempty final assistant message and no input is
/// pending. A contributor that records a terminal outcome for the turn calls
/// `authorize_commit` immediately before its durable commit and rolls back when it returns
/// false: an abort of the turn won. Once it returned true, an abort of the turn waits for the
/// task to end instead of cancelling it, so the recorded outcome is never reported as a
/// cancelled turn. Contributors must not dispatch tools, hooks or model requests here.
pub struct TurnFinalizeInput<'a> {
    /// Stable host-owned turn identifier.
    pub turn_id: &'a str,
    /// The task's final assistant message, exactly as the client received it.
    pub last_agent_message: &'a str,
    /// Authorizes the contributor's durable commit; false means an abort won.
    pub authorize_commit: &'a (dyn Fn() -> bool + Send + Sync),
    /// Store scoped to the host session runtime.
    pub session_store: &'a ExtensionData,
    /// Store scoped to this thread runtime.
    pub thread_store: &'a ExtensionData,
    /// Store scoped to this turn runtime.
    pub turn_store: &'a ExtensionData,
}

/// What a finalizing contributor did with the turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnFinalizeOutcome {
    /// The contributor recorded nothing for this turn.
    NotHandled,
    /// The contributor recorded the turn's terminal outcome.
    Recorded,
    /// The contributor could not establish its outcome; the host shows this warning.
    Warning(String),
}
