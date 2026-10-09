//! The completion call of an action-free run: its exempt commit is deferred to the end of the
//! completing turn (`crate::exempt_completion`).

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_project_intelligence::CompletionFence;
use codex_stateful_runtime::AcceptanceCommit;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunUpdate;
use serde_json::json;

use super::StatefulRunUpdateTool;
use crate::exempt_completion::PendingExemptCompletion;
use crate::tools::MAX_RESPONSE_BYTES;
use crate::tools::bounded_json_output;
use crate::tools::respond;

const PENDING_INSTRUCTION: &str = "The host completes this run under the no-tool exemption when this turn ends. Return submittedResult as the final answer now, without dropping, weakening, or changing any conclusion, caveat, uncertainty, or blocker, and make no further tool call in this turn: any further tool call leaves the run running, and completing it then needs acceptance criteria.";

/// An accepted completion of a run the host recorded no action for.
pub(super) struct ExemptCompletion<'a> {
    pub(super) current: &'a StatefulRun,
    pub(super) update: StatefulRunUpdate,
    pub(super) commit: &'a AcceptanceCommit,
    pub(super) basis: &'a [String],
    pub(super) submitted: Option<&'a str>,
    pub(super) no_reusable_learning: bool,
    pub(super) fence: Option<CompletionFence>,
}

impl StatefulRunUpdateTool {
    /// Leaves the completion pending until its turn ends, or refuses it when finishing it would
    /// itself be an action: publishing durable learning, or a result too large to return here
    /// (reading it back would be a further call). A refused completion records that action.
    pub(super) async fn defer_exempt_completion(
        &self,
        call: &ToolCall<'_>,
        completion: ExemptCompletion<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let ExemptCompletion {
            current,
            update,
            commit,
            basis,
            submitted,
            no_reusable_learning,
            fence,
        } = completion;
        let runtime = self.services.runtime().await.map_err(respond)?;
        runtime
            .end_verification(
                &current.id,
                &commit.verification.owner,
                commit.verification.attempt,
            )
            .await
            .map_err(respond)?;
        if let Some(fence) = fence
            && let Err(error) = fence.release().await
        {
            tracing::warn!("failed to release the Stateful completion fence: {error}");
        }
        let output = json!({
            "runId": current.id.to_string(),
            "status": "running",
            "revision": current.revision,
            "completionPending": true,
            "submittedResult": submitted,
            "acceptanceBasis": basis,
            "finalAnswerInstruction": PENDING_INSTRUCTION,
        });
        let refusal = if !no_reusable_learning {
            Some("a durableLearning completion publishes project knowledge, which is an action")
        } else if output.to_string().len() > call.response_byte_budget(MAX_RESPONSE_BYTES) {
            Some(
                "its result does not fit in this response, and reading it back would be a further call",
            )
        } else {
            None
        };
        if let Some(reason) = refusal {
            runtime
                .record_host_action(&current.id)
                .await
                .map_err(respond)?;
            return Err(FunctionCallError::RespondToModel(format!(
                "completion refused; the run stays running. It cannot finish under the no-tool exemption: {reason}. Record each requirement early with stateful_acceptance_update, settle it, then complete again"
            )));
        }
        crate::exempt_completion::defer(
            call.turn_id.clone(),
            PendingExemptCompletion {
                run_id: current.id.clone(),
                project_id: self.project_id.clone(),
                run_created_at_ms: current.created_at_ms,
                verification_owner: commit.verification.owner.clone(),
                update,
                validated_obligation_sequence: commit.validated_obligation_sequence,
            },
        );
        bounded_json_output(call, output)
    }
}
