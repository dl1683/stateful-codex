use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunUpdate;
use serde::Deserialize;
use serde_json::json;

use crate::StatefulEvent;
use crate::StatefulEventSink;
use crate::completion::HistoricalFindingReference;
use crate::completion::MAX_MATERIAL_HISTORICAL_FINDINGS;
use crate::completion::MAX_MATERIAL_ROOT_FINDINGS;
use crate::completion::prepare_completion;
use crate::services::ProjectIntelligenceServices;

use super::parse_arguments;
use super::respond;
use super::stable_id;
use super::thread_run;

const TOOL_NAME: &str = "stateful_run_update";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    expected_revision: u64,
    status: StatefulRunStatus,
    strategy: Option<String>,
    result: Option<String>,
    root_revision: Option<u64>,
    material_root_findings: Option<Vec<String>>,
    material_historical_findings: Option<Vec<HistoricalFindingArguments>>,
    completion_idempotency_key: Option<String>,
    final_obligation: Option<ObligationPacket>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HistoricalFindingArguments {
    entry_id: String,
    revision: u64,
}

pub(super) struct StatefulRunUpdateTool {
    project_id: String,
    thread_id: String,
    services: ProjectIntelligenceServices,
    event_sink: Option<Arc<dyn StatefulEventSink>>,
}

impl StatefulRunUpdateTool {
    pub(super) fn new(
        project_id: String,
        thread_id: String,
        services: ProjectIntelligenceServices,
        event_sink: Option<Arc<dyn StatefulEventSink>>,
    ) -> Self {
        Self {
            project_id,
            thread_id,
            services,
            event_sink,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let Arguments {
            expected_revision,
            status,
            strategy,
            result,
            root_revision,
            material_root_findings,
            material_historical_findings,
            completion_idempotency_key,
            final_obligation,
        } = parse_arguments(&call)?;
        if !matches!(
            status,
            StatefulRunStatus::Running
                | StatefulRunStatus::Blocked
                | StatefulRunStatus::Completed
                | StatefulRunStatus::Failed
        ) {
            return Err(FunctionCallError::RespondToModel(
                "the model may only update a run to running, blocked, completed, or failed; pause, resume from Socratic pending, and cancel are user controls"
                    .to_string(),
            ));
        }
        let current = thread_run(&self.project_id, &self.thread_id, &self.services).await?;
        if current.status == StatefulRunStatus::Pending {
            return Err(FunctionCallError::RespondToModel(
                "a Socratic run must be resumed explicitly by the user before execution"
                    .to_string(),
            ));
        }
        let runtime = self.services.runtime().await.map_err(respond)?;
        let submitted_result = (status == StatefulRunStatus::Completed)
            .then(|| result.clone())
            .flatten();
        let (completion, final_obligation) = if status == StatefulRunStatus::Completed {
            if current.revision != expected_revision {
                return Err(FunctionCallError::RespondToModel(format!(
                    "run revision conflict: expected {expected_revision}, found {}",
                    current.revision
                )));
            }
            let material_root_findings = material_root_findings.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    format!("completed requires materialRootFindings; pass at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases directly material to the outcome, preserve additional conclusions in the final semantic obligation, or pass [] only after determining no root finding is material")
                )
            })?;
            let material_historical_findings = material_historical_findings
                .unwrap_or_default()
                .into_iter()
                .map(|reference| HistoricalFindingReference {
                    entry_id: reference.entry_id,
                    revision: reference.revision,
                })
                .collect::<Vec<_>>();
            let root_revision = root_revision.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires rootRevision from the current project intelligence World State"
                        .to_string(),
                )
            })?;
            let result = result.as_deref().ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires a final evidence-grounded result".to_string(),
                )
            })?;
            let final_obligation = final_obligation.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires finalObligation with every material conclusion, implication, uncertainty, and blocker needed in the persisted result and final answer"
                        .to_string(),
                )
            })?;
            let completion_idempotency_key = completion_idempotency_key.ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "completed requires completionIdempotencyKey for the final obligation and terminal update"
                        .to_string(),
                )
            })?;
            let completion = prepare_completion(
                &self.project_id,
                &self.services,
                result,
                &final_obligation,
                root_revision,
                &material_root_findings,
                &material_historical_findings,
            )
            .await?;
            let obligation = (
                stable_id("obligation", &self.project_id, &completion_idempotency_key),
                NewObligation {
                    project_id: self.project_id.clone(),
                    run_id: current.id.clone(),
                    packet: final_obligation,
                    provenance_source_id: call.call_id.clone(),
                },
            );
            (Some(completion), Some(obligation))
        } else {
            if material_root_findings.is_some()
                || root_revision.is_some()
                || material_historical_findings.is_some()
                || completion_idempotency_key.is_some()
                || final_obligation.is_some()
            {
                return Err(FunctionCallError::RespondToModel(
                    "rootRevision, materialRootFindings, materialHistoricalFindings, completionIdempotencyKey, and finalObligation are only valid when status is completed".to_string(),
                ));
            }
            (None, None)
        };
        let update = StatefulRunUpdate {
            expected_revision,
            status,
            strategy: strategy.or(current.strategy),
            result: completion
                .as_ref()
                .map(|completion| completion.result.clone())
                .or(result)
                .or(current.result),
        };
        let (run, final_obligation) = if let Some((obligation_id, obligation)) = final_obligation {
            let (run, obligation) = runtime
                .complete_run_with_obligation(&current.id, update, obligation_id, obligation)
                .await
                .map_err(respond)?;
            (run, Some(obligation))
        } else {
            (
                runtime
                    .update_run(&current.id, update)
                    .await
                    .map_err(respond)?,
                None,
            )
        };
        if let Some(event_sink) = &self.event_sink {
            if let Some(obligation) = &final_obligation {
                event_sink.emit(StatefulEvent::ObligationUpdated {
                    project_id: obligation.value.project_id.clone(),
                    run_id: obligation.value.run_id.to_string(),
                    obligation_id: obligation.id.clone(),
                    revision: obligation.revision,
                });
            }
            event_sink.emit(StatefulEvent::RunUpdated {
                project_id: run.value.project_id.clone(),
                run_id: run.id.to_string(),
                revision: run.revision,
            });
        }
        let final_answer_checklist = completion
            .as_ref()
            .map(|completion| completion.checklist.clone())
            .unwrap_or_default();
        let omitted_checklist_items = completion
            .as_ref()
            .map_or(0, |completion| completion.omitted_checklist_items);
        Ok(Box::new(JsonToolOutput::new(json!({
            "runId": run.id.to_string(),
            "status": status_name(run.status),
            "revision": run.revision,
            "strategyRevision": run.strategy_revision,
            "finalAnswerChecklist": final_answer_checklist,
            "omittedChecklistItems": omitted_checklist_items,
            "submittedResult": submitted_result,
            "finalAnswerInstruction": (run.status == StatefulRunStatus::Completed).then_some(
                "Return submittedResult as the final answer without dropping, weakening, or changing any conclusion, caveat, uncertainty, or blocker. You may improve formatting and exact-source links. Copy opaque evidence identifiers only from finalAnswerChecklist; never reconstruct or abbreviate them from memory. Use finalAnswerChecklist to confirm that the visible answer preserves the durable completion basis; if omittedChecklistItems is nonzero, also use finalObligation from this call."
            ),
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for StatefulRunUpdateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: format!("Persist a meaningful strategy/status change or final evidence-grounded result for the selected thread's active Stateful run. expectedRevision is the current run revision, while rootRevision is the separate project intelligence revision. Completed records finalObligation and the terminal result together in one atomic storage transaction: finish every blackboard, relationship, steering, and verification operation first; select at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority current E aliases and at most {MAX_MATERIAL_HISTORICAL_FINDINGS} exact historical entry revisions directly material to the outcome; then supply completionIdempotencyKey, finalObligation, and the result in this single final Stateful mutation. Full source fingerprints in result or finalObligation must belong to a selected current or historical finding; the host renders selected evidence identifiers into the durable completion basis. The tool rejects changed run or root revisions before persistence and appends the selected findings and bounded final-obligation conclusions to the durable result. This cannot bypass a pending Socratic run or perform user-owned pause/cancel controls."),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "expectedRevision": {"type": "integer", "minimum": 1, "description": "Copy the Run revision from the Stateful run World State. This is not the project intelligence revision used by rootRevision."},
                    "status": {"type": "string", "enum": ["running", "blocked", "completed", "failed"], "description": "Use completed only after all other durable writes are finished; the same call must carry the final semantic obligation and removes the active-run binding."},
                    "strategy": {"type": "string"},
                    "result": {"type": "string", "description": "For completed, the concise final evidence-grounded narrative after all durable writes and verification. The tool appends the structured completion basis."},
                    "rootRevision": {"type": "integer", "minimum": 0, "description": "Required for completed. Copy the project intelligence revision shown with the current root blackboard; completion fails before mutation if it changed."},
                    "materialRootFindings": {"type": "array", "items": {"type": "string", "pattern": "^E[1-9][0-9]*$"}, "maxItems": MAX_MATERIAL_ROOT_FINDINGS, "description": format!("Required for completed. Select at most {MAX_MATERIAL_ROOT_FINDINGS} highest-priority E aliases shown at rootRevision that are directly material to the requested outcome; preserve additional material conclusions in finalObligation. Use [] only after determining no root finding is material.")},
                    "materialHistoricalFindings": {"type": "array", "items": {"type": "object", "properties": {"entryId": {"type": "string"}, "revision": {"type": "integer", "minimum": 1}}, "required": ["entryId", "revision"], "additionalProperties": false}, "maxItems": MAX_MATERIAL_HISTORICAL_FINDINGS, "description": format!("Optional for completed. Select at most {MAX_MATERIAL_HISTORICAL_FINDINGS} exact superseded or tombstoned entry IDs and revisions returned by blackboard_query when historical evidence is material. The host renders their stored source fingerprints; do not copy opaque fingerprints manually.")},
                    "completionIdempotencyKey": {"type": "string", "description": "Required for completed. Reuse only when retrying this identical final obligation and terminal result."},
                    "finalObligation": super::obligation::obligation_packet_schema()
                },
                "required": ["expectedRevision", "status"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static run update schema: {error}")),
            output_schema: None,
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

fn status_name(status: StatefulRunStatus) -> &'static str {
    match status {
        StatefulRunStatus::Pending => "pending",
        StatefulRunStatus::Running => "running",
        StatefulRunStatus::Paused => "paused",
        StatefulRunStatus::Completed => "completed",
        StatefulRunStatus::Cancelled => "cancelled",
        StatefulRunStatus::Blocked => "blocked",
        StatefulRunStatus::Failed => "failed",
    }
}
use std::sync::Arc;
