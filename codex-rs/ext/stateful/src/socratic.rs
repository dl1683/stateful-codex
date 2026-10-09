use codex_extension_api::ExtensionFuture;
use codex_extension_api::ToolPolicyContributor;
use codex_extension_api::ToolPolicyDecision;
use codex_extension_api::ToolPolicyInput;
use codex_extension_api::UserShellPolicyInput;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;

use crate::SelectedProject;
use crate::StatefulExtension;

impl ToolPolicyContributor for StatefulExtension {
    fn evaluate<'a>(
        &'a self,
        input: ToolPolicyInput<'a>,
    ) -> ExtensionFuture<'a, ToolPolicyDecision> {
        Box::pin(async move {
            let (Some(selected), Some(services)) = (
                input.thread_store.get::<SelectedProject>(),
                self.services.as_ref(),
            ) else {
                return ToolPolicyDecision::Allow;
            };
            let store = match services.runtime().await {
                Ok(store) => store,
                Err(error) => {
                    tracing::warn!(%error, "failed to open Socratic run policy store");
                    return ToolPolicyDecision::Allow;
                }
            };
            let run = match store.run_for_thread(&input.thread_id.to_string()).await {
                Ok(Some(run)) if run.value.project_id == selected.project_id() => run,
                Ok(_) => return ToolPolicyDecision::Allow,
                Err(error) => {
                    tracing::warn!(thread_id = %input.thread_id, %error, "failed to load Socratic run policy");
                    return ToolPolicyDecision::Allow;
                }
            };
            if run.value.mode != WorkflowMode::Socratic
                || run.status != StatefulRunStatus::Pending
                || allowed_while_pending(input.tool_name)
            {
                return ToolPolicyDecision::Allow;
            }
            ToolPolicyDecision::Block {
                reason: format!(
                    "tool {} is blocked while Socratic run {} is pending; ask and synthesize first, then wait for the user to explicitly resume the run",
                    input.tool_name, run.id
                ),
            }
        })
    }

    /// A user shell command is an action of the open runs durably bound to the thread,
    /// whatever project the thread currently shows: it is recorded before it is spawned, and
    /// refused when the record cannot be written.
    fn evaluate_user_shell<'a>(
        &'a self,
        input: UserShellPolicyInput<'a>,
    ) -> ExtensionFuture<'a, ToolPolicyDecision> {
        Box::pin(async move {
            let Some(services) = self.services.as_ref() else {
                return ToolPolicyDecision::Allow;
            };
            match crate::host_actions::record_user_shell(services, &input.thread_id.to_string())
                .await
            {
                Ok(()) => ToolPolicyDecision::Allow,
                Err(error) => ToolPolicyDecision::Block {
                    reason: format!(
                        "the shell command was not run: the host could not durably record it against this thread's Stateful run first ({error})"
                    ),
                },
            }
        })
    }
}

fn allowed_while_pending(tool_name: &codex_extension_api::ToolName) -> bool {
    tool_name.is_default_namespace()
        && matches!(
            tool_name.name.as_str(),
            "request_user_input"
                | "request_user_input_async"
                | "send_user_message_async"
                | "blackboard_query"
                | "memory_read"
                | "context_map_query"
                | "obligation_update"
                | "stateful_run_read"
                | "steering_query"
                | "steering_reconcile"
                // Discovery only: tools it loads are still checked here before they run.
                | "tool_search"
        )
}
