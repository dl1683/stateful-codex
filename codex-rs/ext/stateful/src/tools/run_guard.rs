//! Decoding and bounded, actionable rejections for `stateful_run_update`.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;

use super::Arguments;
use super::CompletionDisposition;
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
pub(super) const ANSWER_GUIDANCE: &str = "Give a substantive final answer; an eligible final ends Answered or Partial. Neither outcome certifies work; no completion call is required. If you recorded next work this turn, explicitly choose the answer with this trailing block on separate lines:\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]\nList unfinished items under open-issues instead of none. Apply or settle pending steering first; size, binding and authorization gates still apply.";

#[derive(Clone, Copy)]
enum RejectionRoute {
    Answer,
    Verification,
}

impl StatefulRunUpdateTool {
    /// Decodes the arguments once, runs the update, and returns every rejection within the
    /// call's serialized allowance. Guidance follows the run mode, never the error's prose.
    pub(super) async fn handle_guided(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let route = match thread_run(&self.project_id, &self.thread_id, &self.services).await {
            Ok(run) => match run.value.mode {
                WorkflowMode::Autonomous => {
                    let runtime = self.services.runtime().await.map_err(super::respond)?;
                    let ledger = runtime
                        .acceptance_ledger(&run.id)
                        .await
                        .map_err(super::respond)?;
                    let blockers = runtime
                        .latest_obligation(&run.id)
                        .await
                        .map_err(super::respond)?;
                    if ledger.criteria.iter().any(|criterion| {
                        criterion.required
                            && criterion.state == codex_stateful_runtime::AcceptanceState::Active
                    }) && blockers.is_none_or(|row| row.value.packet.blockers.is_empty())
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
            Err(_) => RejectionRoute::Verification,
        };
        let example = match route {
            RejectionRoute::Answer => ANSWER_GUIDANCE.to_string(),
            RejectionRoute::Verification => format!(
                "Argument shapes only; verification and terminal gates still apply. {EXAMPLE}"
            ),
        };
        let arguments: Arguments = match parse_arguments(&call, &example) {
            Ok(arguments) => arguments,
            Err(FunctionCallError::RespondToModel(message)) => {
                let message = match route {
                    RejectionRoute::Answer => {
                        message.replace("Minimal valid call: ", "Final-answer guidance: ")
                    }
                    RejectionRoute::Verification => message,
                };
                return Err(bounded_rejection(
                    &call, &message, /*guide*/ None, route,
                ));
            }
            Err(error) => return Err(error),
        };
        let completing = arguments.status == StatefulRunStatus::Completed;
        // Only known disposition-shape errors retain verification templates in Autonomous.
        // Evidence refusals always give the qualified final-answer escape.
        let shape_error = arguments.completion_disposition
            == CompletionDisposition::NoReusableLearning
            && arguments.open_issues.as_ref().is_none_or(Vec::is_empty)
            && arguments
                .final_obligation
                .as_ref()
                .is_none_or(|packet| packet.blockers.is_empty())
            && (arguments.root_revision.is_some()
                || arguments.material_root_findings.is_some()
                || arguments.completion_idempotency_key.is_some()
                || arguments.final_obligation.is_some());
        let route = if matches!(route, RejectionRoute::Verification)
            && !shape_error
            && thread_run(&self.project_id, &self.thread_id, &self.services)
                .await
                .is_ok_and(|run| run.value.mode == WorkflowMode::Autonomous)
        {
            RejectionRoute::Answer
        } else {
            route
        };
        let reply = call.clone();
        match self.handle_call(call, arguments).await {
            Err(FunctionCallError::RespondToModel(message)) => {
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

    /// Copyable argument shapes; these establish neither evidence nor terminal eligibility.
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
