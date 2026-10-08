//! Guidance and the per-turn retry bound for rejected `stateful_run_update` completions.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::ObligationPacket;
use serde_json::Value;

use super::StatefulRunUpdateTool;
use super::status_name;
use crate::StatefulEvent;
use crate::completion::completion_root;
use crate::completion_attempts::MAX_REJECTED_COMPLETIONS_PER_TURN;
use crate::tools::MAX_RESPONSE_BYTES;
use crate::tools::provenance_source_id;
use crate::tools::receipt_error;
use crate::tools::stable_id;
use crate::tools::thread_run;

impl StatefulRunUpdateTool {
    /// Runs one call; a rejected completion attempt also shows valid completion calls
    /// filled with the run's current revisions, so the next attempt can succeed. After
    /// `MAX_REJECTED_COMPLETIONS_PER_TURN` rejected attempts in a turn, completion stops:
    /// the run stays open with the unrecorded completion as a blocker, the model is told
    /// to answer without completing, and this tool is withheld for the rest of the turn.
    pub(super) async fn handle_guided(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let completing = is_completion_attempt(&call);
        let attempts = self.services.completion_attempts();
        let turn_id = call.turn_id.clone();
        let call_id = call.call_id.clone();
        // A call already dispatched before the tool was withheld does no further work.
        if completing
            && attempts.rejected(&self.thread_id, &turn_id) >= MAX_REJECTED_COMPLETIONS_PER_TURN
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "Stateful completion is closed for this turn after {MAX_REJECTED_COMPLETIONS_PER_TURN} rejected attempts and was not recorded. Do not call stateful_run_update again; give the final answer now and say that the Stateful run was not completed."
            )));
        }
        let budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let fit = |message: String, fallback: String| {
            if serde_json::to_string(&message).is_ok_and(|text| text.len() <= budget) {
                message
            } else {
                fallback
            }
        };
        match self.handle_call(call).await {
            Ok(output) => {
                if completing {
                    attempts.clear(&self.thread_id);
                }
                Ok(output)
            }
            Err(FunctionCallError::RespondToModel(message)) if completing => {
                let rejected = attempts.reject(&self.thread_id, &turn_id);
                if rejected >= MAX_REJECTED_COMPLETIONS_PER_TURN {
                    let stop = self.stop_completion(&turn_id, &call_id, &message).await;
                    return Err(FunctionCallError::RespondToModel(fit(
                        stop,
                        format!(
                            "Stateful completion was not recorded after {rejected} rejected attempts and is closed for this turn. Do not call stateful_run_update again; give the final answer now and say the run was not completed."
                        ),
                    )));
                }
                let guided = format!(
                    "{message}. {} Completion attempt {rejected} of {MAX_REJECTED_COMPLETIONS_PER_TURN} in this turn.",
                    self.completion_guide().await
                );
                Err(FunctionCallError::RespondToModel(fit(guided, message)))
            }
            outcome => outcome,
        }
    }

    /// Ends completion for this turn truthfully: records the unrecorded completion as a
    /// blocker on the still-open run and returns the instruction for the model.
    async fn stop_completion(&self, turn_id: &str, call_id: &str, rejection: &str) -> String {
        let rejection = receipt_error(rejection);
        let stop = "Completion is closed for the rest of this turn, so do not call stateful_run_update again. Give the user your final answer now and say that the Stateful run was not completed.";
        let run = match thread_run(&self.project_id, &self.thread_id, &self.services).await {
            Ok(run) if run.status.is_terminal() => {
                return format!(
                    "The Stateful run is already {}; it needs no further update. {stop}",
                    status_name(run.status)
                );
            }
            Ok(run) => run,
            Err(_) => {
                return format!(
                    "Stateful completion was NOT recorded: {MAX_REJECTED_COMPLETIONS_PER_TURN} completion attempts were rejected in this turn; the last said: {rejection} {stop}"
                );
            }
        };
        let blocker = NewObligation {
            project_id: self.project_id.clone(),
            run_id: run.id.clone(),
            packet: ObligationPacket {
                blockers: vec![format!(
                    "Stateful completion was not recorded in turn {turn_id}: {MAX_REJECTED_COMPLETIONS_PER_TURN} completion attempts were rejected; the last said: {rejection}"
                )],
                next: vec![
                    "Complete the run with one valid stateful_run_update in a later turn."
                        .to_string(),
                ],
                ..ObligationPacket::default()
            },
            provenance_source_id: provenance_source_id(call_id),
        };
        let recorded = match self.services.runtime().await {
            Ok(runtime) => runtime
                .append_obligation(
                    stable_id(
                        "obligation",
                        &self.project_id,
                        &format!("completion-unrecorded:{}:{turn_id}", run.id),
                    ),
                    blocker,
                )
                .await
                .ok(),
            Err(_) => None,
        };
        if let (Some(obligation), Some(event_sink)) = (&recorded, &self.event_sink) {
            event_sink.emit(StatefulEvent::ObligationUpdated {
                project_id: obligation.value.project_id.clone(),
                run_id: obligation.value.run_id.to_string(),
                obligation_id: obligation.id.clone(),
                revision: obligation.revision,
            });
        }
        let record = if recorded.is_some() {
            "The run stays open and incomplete, with this recorded as a blocker."
        } else {
            "The run stays open and incomplete."
        };
        format!(
            "Stateful completion was NOT recorded: {MAX_REJECTED_COMPLETIONS_PER_TURN} completion attempts were rejected in this turn; the last said: {rejection} {record} {stop}"
        )
    }

    /// Valid completion calls for the run's current state, as copyable JSON.
    async fn completion_guide(&self) -> String {
        let Ok(run) = thread_run(&self.project_id, &self.thread_id, &self.services).await else {
            return "Completion needs an active Stateful run on this thread.".to_string();
        };
        let revision = run.revision;
        let lookup = format!(
            r#"noReusableLearning (only when this run wrote no project knowledge): {{"expectedRevision":{revision},"status":"completed","completionDisposition":"noReusableLearning","result":"<final answer>"}}"#
        );
        let durable = match completion_root(&self.services, &self.project_id, &self.thread_id)
            .await
        {
            Ok((root_revision, aliases)) if aliases > 0 => format!(
                r#"durableLearning: {{"expectedRevision":{revision},"status":"completed","completionIdempotencyKey":"completion-{revision}","finalObligation":{{"learning":["<reusable conclusion>"]}},"result":"<final answer>","rootRevision":{root_revision},"materialRootFindings":["E1"]}} (E1..E{aliases} are the current root aliases)"#
            ),
            Ok(_) => "durableLearning first needs a root finding: record it with blackboard_record_batch rootPromotion promoted".to_string(),
            Err(_) => "durableLearning needs rootRevision and materialRootFindings from the current World State".to_string(),
        };
        format!("Valid completion calls (replace the <...> text): {lookup}; {durable}.")
    }
}

/// Whether a call tries to complete the run. Undecodable arguments count too: a model
/// repeating a malformed call is not converging either.
fn is_completion_attempt(call: &ToolCall<'_>) -> bool {
    call.function_arguments()
        .ok()
        .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok())
        .is_none_or(|arguments| arguments["status"] == "completed")
}
