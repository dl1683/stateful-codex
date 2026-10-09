use codex_protocol::ThreadId;
use codex_tools::ToolCallSource;
use codex_tools::ToolName;

use crate::ExtensionData;
use crate::ExtensionFuture;

/// Immutable tool identity supplied before a model-requested tool executes.
pub struct ToolPolicyInput<'a> {
    pub thread_id: ThreadId,
    pub session_store: &'a ExtensionData,
    pub thread_store: &'a ExtensionData,
    pub turn_store: &'a ExtensionData,
    pub turn_id: &'a str,
    pub tool_name: &'a ToolName,
    pub source: ToolCallSource,
}

/// Identity of a user-initiated shell command (not a model tool call), supplied before the
/// host spawns it, during a turn or between turns.
pub struct UserShellPolicyInput<'a> {
    pub thread_id: ThreadId,
    pub session_store: &'a ExtensionData,
    pub thread_store: &'a ExtensionData,
    pub turn_store: &'a ExtensionData,
    pub turn_id: &'a str,
}

/// Decision returned by one extension tool policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolPolicyDecision {
    Allow,
    Block { reason: String },
}

/// Extension policy gate evaluated after hook rewrites and before execution.
///
/// All contributors must allow a call. Implementations should decide from
/// trusted host state and tool identity, and must not log sensitive payloads.
pub trait ToolPolicyContributor: Send + Sync {
    fn evaluate<'a>(
        &'a self,
        input: ToolPolicyInput<'a>,
    ) -> ExtensionFuture<'a, ToolPolicyDecision>;

    /// Evaluated before the host spawns a user shell command. All contributors must allow
    /// it; a blocked command never starts. Contributors without a user-shell policy allow.
    fn evaluate_user_shell<'a>(
        &'a self,
        _input: UserShellPolicyInput<'a>,
    ) -> ExtensionFuture<'a, ToolPolicyDecision> {
        Box::pin(std::future::ready(ToolPolicyDecision::Allow))
    }
}
