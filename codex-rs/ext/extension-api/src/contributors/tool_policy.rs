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
}
