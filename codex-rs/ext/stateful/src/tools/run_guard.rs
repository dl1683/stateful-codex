//! Decoding and bounded, actionable rejections for `stateful_run_update`.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;

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
const ANSWER_FALLBACK: &str = "stateful_run_update was rejected; read the run. Give a substantive final answer; an eligible final ends Answered or Partial. Apply or settle pending steering first.";
pub(super) const ANSWER_GUIDANCE: &str = "Give a substantive final answer; an eligible final ends Answered or Partial. Neither certifies work; no completion call is required. After recorded next work, explicitly choose the answer with these trailer lines:\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]\nFor unfinished items, use open-issues: followed by separate lines starting with '- '. Apply or settle pending steering first; size, binding and authorization gates still apply.";

#[derive(Clone, Copy)]
enum RejectionRoute {
    Answer,
    Verification,
}

#[derive(Clone, Copy)]
pub(super) enum RejectionCategory {
    Structural,
    Evidence,
}

impl StatefulRunUpdateTool {
    /// Decodes the arguments once, runs the update, and returns every rejection within the
    /// call's serialized allowance. Guidance follows the run mode, never the error's prose.
    pub(super) async fn handle_guided(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let reject =
            |error: &str| bounded_rejection(&call, error, None, RejectionRoute::Verification);
        let mut autonomous = false;
        let incoming_issues = call
            .function_arguments()
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            .is_some_and(|value| {
                [
                    value.get("openIssues"),
                    value.pointer("/finalObligation/blockers"),
                ]
                .into_iter()
                .flatten()
                .any(|items| items.as_array().is_some_and(|items| !items.is_empty()))
            });
        let route = match thread_run(&self.project_id, &self.thread_id, &self.services).await {
            Ok(run) => match run.value.mode {
                WorkflowMode::Autonomous => {
                    autonomous = true;
                    let runtime = self
                        .services
                        .runtime()
                        .await
                        .map_err(|error| reject(&error.to_string()))?;
                    let ledger = runtime
                        .acceptance_ledger(&run.id)
                        .await
                        .map_err(|error| reject(&error.to_string()))?;
                    let blockers = runtime
                        .latest_obligation(&run.id)
                        .await
                        .map_err(|error| reject(&error.to_string()))?;
                    if ledger.criteria.iter().any(|criterion| {
                        criterion.required
                            && criterion.state == codex_stateful_runtime::AcceptanceState::Active
                    }) && !incoming_issues
                        && blockers.is_none_or(|row| row.value.packet.blockers.is_empty())
                    {
                        RejectionRoute::Verification
                    } else {
                        RejectionRoute::Answer
                    }
                }
                WorkflowMode::Collaborative | WorkflowMode::Socratic => {
                    RejectionRoute::Verification
                }
            },
            Err(error) => return Err(reject(&error.to_string())),
        };
        let arguments: Arguments = match parse_arguments(&call, "") {
            Ok(arguments) => arguments,
            Err(FunctionCallError::RespondToModel(message)) => {
                let message = message
                    .strip_suffix(". Minimal valid call: ")
                    .unwrap_or(&message);
                let guide = match route {
                    RejectionRoute::Answer => format!("Final-answer guidance: {ANSWER_GUIDANCE}"),
                    RejectionRoute::Verification => format!(
                        "Argument shapes only; verification and terminal gates still apply. {EXAMPLE}"
                    ),
                };
                let guide_fits = serde_json::to_string(&format!("{message}. {guide}"))
                    .is_ok_and(|json| json.len() <= call.response_byte_budget(MAX_RESPONSE_BYTES));
                let message =
                    if autonomous && !guide_fits && matches!(route, RejectionRoute::Answer) {
                        ANSWER_FALLBACK
                    } else {
                        message
                    };
                return Err(bounded_rejection(&call, message, Some(&guide), route));
            }
            Err(error) => return Err(error),
        };
        let completing = arguments.status == StatefulRunStatus::Completed;
        let mut category = RejectionCategory::Evidence;
        let reply = call.clone();
        match self.handle_call(call, arguments, &mut category).await {
            Err(FunctionCallError::RespondToModel(message)) => {
                let route = if autonomous && matches!(category, RejectionCategory::Evidence) {
                    RejectionRoute::Answer
                } else {
                    route
                };
                let guide = match (completing, route) {
                    (true, RejectionRoute::Answer) => Some(ANSWER_GUIDANCE.to_string()),
                    (true, RejectionRoute::Verification) => Some(self.completion_guide().await),
                    (false, RejectionRoute::Answer | RejectionRoute::Verification) => None,
                };
                Err(bounded_rejection(&reply, &message, guide.as_deref(), route))
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
        format!(
            "Argument shapes only; verification and terminal gates still apply. Replace the <...> text: {lookup}; {durable}."
        )
    }
}

/// The final model-facing rejection: the message with its guidance when both fit the call's
/// serialized allowance, else the message alone, else a compact statement; always passed
/// through `bounded_respond`, so no branch can exceed the allowance.
fn bounded_rejection(
    call: &ToolCall<'_>,
    message: &str,
    guide: Option<&str>,
    route: RejectionRoute,
) -> FunctionCallError {
    let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
    let fits = |text: &str| serde_json::to_string(text).is_ok_and(|json| json.len() <= budget);
    let guided = guide.map(|guide| format!("{message}. {guide}"));
    let chosen = if let Some(guided) = guided
        && fits(&guided)
    {
        guided
    } else if fits(message) {
        message.to_string()
    } else {
        match route {
            RejectionRoute::Answer => ANSWER_FALLBACK.to_string(),
            RejectionRoute::Verification => OVERSIZED_REJECTION.to_string(),
        }
    };
    bounded_respond(call, &chosen)
}

#[cfg(test)]
#[path = "run_guard_tests.rs"]
mod tests;
