//! Decoding and bounded, actionable rejections for `stateful_run_update`.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_stateful_runtime::StatefulRunStatus;

use super::Arguments;
use super::EXAMPLE;
use super::StatefulRunUpdateTool;
use crate::completion::completion_root;
use crate::tools::MAX_RESPONSE_BYTES;
use crate::tools::bounded_respond;
use crate::tools::parse_arguments;
use crate::tools::thread_run;

/// Returned when a rejection, even without its guidance, exceeds the call's allowance. It
/// claims nothing about state, because some rejections follow a committed update.
const OVERSIZED_REJECTION: &str = "stateful_run_update was rejected with a message longer than this call's response allowance; read the run with stateful_run_read before retrying.";

impl StatefulRunUpdateTool {
    /// Decodes the arguments once, runs the update, and returns every rejection within the
    /// call's serialized allowance. A rejected completion also shows valid completion calls
    /// filled with the run's current revisions, so the next attempt can succeed.
    pub(super) async fn handle_guided(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: Arguments = parse_arguments(&call, EXAMPLE)?;
        let completing = arguments.status == StatefulRunStatus::Completed;
        let reply = call.clone();
        match self.handle_call(call, arguments).await {
            Err(FunctionCallError::RespondToModel(message)) => {
                let guide = if completing {
                    Some(self.completion_guide().await)
                } else {
                    None
                };
                Err(bounded_rejection(&reply, &message, guide.as_deref()))
            }
            outcome => outcome,
        }
    }

    /// Valid completion calls for the run's current state, as copyable JSON.
    async fn completion_guide(&self) -> String {
        let Ok(run) = thread_run(&self.project_id, &self.thread_id, &self.services).await else {
            return "Completion needs an active Stateful run on this thread.".to_string();
        };
        let revision = run.revision;
        let lookup = format!(
            r#"noReusableLearning (only when this run wrote no project knowledge): {{"expectedRevision":{revision},"status":"completed","completionDisposition":"noReusableLearning","result":"<final answer>","openIssues":[]}}"#
        );
        let durable = match completion_root(&self.services, &self.project_id, &self.thread_id)
            .await
        {
            Ok((root_revision, aliases)) if aliases > 0 => format!(
                r#"durableLearning: {{"expectedRevision":{revision},"status":"completed","completionIdempotencyKey":"<unique completion key for this run>","finalObligation":{{"learning":["<reusable conclusion>"]}},"result":"<final answer>","rootRevision":{root_revision},"materialRootFindings":["E1"],"openIssues":[]}} (E1..E{aliases} are the current root aliases)"#
            ),
            Ok(_) => "durableLearning first needs a root finding: record it with blackboard_record_batch rootPromotion promoted".to_string(),
            Err(_) => "durableLearning needs rootRevision and materialRootFindings from the current World State".to_string(),
        };
        format!("Valid completion calls (replace the <...> text): {lookup}; {durable}.")
    }
}

/// The final model-facing rejection: the message with its guidance when both fit the call's
/// serialized allowance, else the message alone, else a compact statement; always passed
/// through `bounded_respond`, so no branch can exceed the allowance.
fn bounded_rejection(call: &ToolCall<'_>, message: &str, guide: Option<&str>) -> FunctionCallError {
    let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
    let fits = |text: &str| serde_json::to_string(text).is_ok_and(|json| json.len() <= budget);
    let guided = guide.map(|guide| format!("{message}. {guide}"));
    let chosen = match guided {
        Some(guided) if fits(&guided) => guided,
        _ if fits(message) => message.to_string(),
        _ => OVERSIZED_REJECTION.to_string(),
    };
    bounded_respond(call, &chosen)
}

#[cfg(test)]
#[path = "run_guard_tests.rs"]
mod tests;
