//! Autonomous completion redirects never certify work or cause host-induced blocking.

use super::StatefulRunUpdateTool;
use super::bounded_json_output;
use super::provenance_source_id;
use super::respond;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolOutput;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::ObligationOperation;
use codex_stateful_runtime::OperationReceiptKey;
use codex_stateful_runtime::RETAINED_ISSUE_PREFIX;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use serde_json::json;

// Keep this new model-visible receipt below 1K tokens even at one UTF-8 byte per token.
const MAX_REDIRECT_RECEIPT_BYTES: usize = 900;

impl StatefulRunUpdateTool {
    pub(super) async fn autonomous_completion_redirect(
        &self,
        call: &ToolCall<'_>,
        current: &StatefulRun,
        expected_revision: u64,
        raw: &[String],
        display: &[String],
        retained: &[String],
    ) -> Result<Option<Box<dyn ToolOutput>>, FunctionCallError> {
        if current.status != StatefulRunStatus::Running || expected_revision != current.revision {
            return Err(FunctionCallError::RespondToModel(
                "completed requires this run to be running at the expected revision; nothing changed".into(),
            ));
        }
        let runtime = self.services.runtime().await.map_err(respond)?;
        let ledger = runtime
            .acceptance_ledger(&current.id)
            .await
            .map_err(respond)?;
        // Omission criteria become Active only after acceptance; proposals are not contracts.
        let declared = ledger
            .criteria
            .iter()
            .any(|criterion| criterion.required && criterion.state == AcceptanceState::Active);
        if declared && display.is_empty() && retained.is_empty() {
            return Ok(None);
        }
        let instruction = if declared {
            "Open issues are retained. Give your answer as your final message now; the host records it as partial, not verified, listing open or not-certified items."
        } else {
            "This run declared no acceptance criteria, so it cannot be verified as completed. Give your answer as your final message now; the host records it as answered, not verified, or partial while issues are retained."
        };
        // Reserve the fixed receipt before writing. Lists can exceed a model item even
        // within the operation's caps; omitted entries remain in the durable obligation.
        let mut output = json!({
            "runId": current.id.to_string(), "status": "running", "revision": current.revision,
            "retainedIssues": [], "omittedRetainedIssues": 32,
            "retainedIssuesRead": "stateful_run_read section obligation",
            "instruction": instruction,
        });
        let budget = call
            .response_byte_budget(super::super::MAX_RESPONSE_BYTES)
            .min(MAX_REDIRECT_RECEIPT_BYTES);
        if output.to_string().len() > budget {
            return Err(super::super::bounded_respond(
                call,
                "the bounded completion redirect receipt cannot fit; nothing changed; give the final answer directly, or read the run first",
            ));
        }
        let issues: Vec<String> = raw.iter().map(|issue| issue.trim().to_string()).collect();
        let retained = if issues.is_empty() {
            retained.to_vec()
        } else {
            let source = provenance_source_id(call)?;
            let obligation = runtime
                .apply_obligation_operation(
                    &current.id,
                    ObligationOperation::Retain { issues },
                    &OperationReceiptKey::parse(source.clone()).map_err(respond)?,
                    &source,
                )
                .await
                .map_err(respond)?;
            if let Some(sink) = &self.event_sink {
                sink.emit(crate::StatefulEvent::ObligationUpdated {
                    project_id: obligation.value.project_id.clone(),
                    run_id: current.id.to_string(),
                    obligation_id: obligation.id.clone(),
                    revision: obligation.revision,
                });
            }
            obligation
                .value
                .packet
                .blockers
                .into_iter()
                .filter(|issue| issue.starts_with(RETAINED_ISSUE_PREFIX))
                .collect()
        };
        let mut retained = retained;
        let total = retained.len();
        output["retainedIssues"] = json!(retained);
        output["omittedRetainedIssues"] = json!(0);
        while output.to_string().len() > budget {
            retained.pop();
            output["retainedIssues"] = json!(retained);
            output["omittedRetainedIssues"] = json!(total - retained.len());
        }
        Ok(Some(bounded_json_output(call, output)?))
    }
}
