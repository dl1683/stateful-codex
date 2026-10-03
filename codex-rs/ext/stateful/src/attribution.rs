use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use codex_extension_api::ExtensionData;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolFinishInput;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::ToolLifecycleFuture;
use codex_extension_api::ToolPayload;
use codex_extension_api::ToolStartInput;
use serde::Deserialize;

use crate::SelectedProject;
use crate::SelectedThread;
use crate::StatefulEvent;
use crate::StatefulExtension;
use crate::source_freshness::EvidenceAudit;
use codex_stateful_runtime::StatefulRunId;

const BLACKBOARD_QUERY: &str = "blackboard_query";
const CONTEXT_MAP_QUERY: &str = "context_map_query";
const EVIDENCE_READ: &str = "evidence_read";
const STEERING_QUERY: &str = "steering_query";
const CONVERSATION_READ: &str = "conversation_read";
const MEMORY_READ: &str = "memory_read";
const BLACKBOARD_BATCH_RECORD: &str = "blackboard_record_batch";
const BLACKBOARD_UPDATE: &str = "blackboard_update_batch";
const BLACKBOARD_RELATE: &str = "blackboard_relate";
const CONTEXT_MAP_REFRESH: &str = "context_map_refresh";
const OBLIGATION_UPDATE: &str = "obligation_update";
const STATEFUL_RUN_UPDATE: &str = "stateful_run_update";
const STATEFUL_RUN_READ: &str = "stateful_run_read";
const STEERING_RECONCILE: &str = "steering_reconcile";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatefulAttributionStatus {
    Completed,
    Failed,
    Aborted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatefulAttributionCounters {
    pub world_state_samples: u64,
    pub root_entries_loaded: u64,
    pub root_evidence_routes_checked: u64,
    pub root_evidence_routes_current: u64,
    pub root_evidence_routes_stale: u64,
    pub root_evidence_routes_unavailable: u64,
    pub root_evidence_routes_unchecked: u64,
    pub root_unique_sources_observed: u64,
    pub root_source_bytes_hashed: u64,
    pub stateful_tool_calls: u64,
    pub failed_stateful_tool_calls: u64,
    pub knowledge_query_calls: u64,
    pub route_query_calls: u64,
    pub evidence_read_calls: u64,
    pub steering_query_calls: u64,
    pub conversation_read_calls: u64,
    pub blackboard_write_calls: u64,
    pub context_refresh_calls: u64,
    pub obligation_write_calls: u64,
    pub run_update_calls: u64,
    pub steering_write_calls: u64,
    pub material_findings_reused: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulAttributionSummary {
    pub run_id: Option<StatefulRunId>,
    pub project_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub status: StatefulAttributionStatus,
    pub duration_ms: u64,
    pub counters: StatefulAttributionCounters,
}

#[derive(Clone, Default)]
pub(super) struct StatefulAttributionTracker {
    turns: Arc<Mutex<HashMap<String, ActiveAttribution>>>,
}

struct ActiveAttribution {
    run_id: Option<StatefulRunId>,
    project_id: String,
    thread_id: String,
    started_at: Instant,
    failed: bool,
    counters: StatefulAttributionCounters,
    pending_material_findings: HashMap<String, u64>,
}

impl StatefulAttributionTracker {
    pub(super) fn begin(&self, turn_id: &str, project_id: String, thread_id: String) {
        self.turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                turn_id.to_string(),
                ActiveAttribution {
                    run_id: None,
                    project_id,
                    thread_id,
                    started_at: Instant::now(),
                    failed: false,
                    counters: StatefulAttributionCounters::default(),
                    pending_material_findings: HashMap::new(),
                },
            );
    }

    pub(super) fn bind_run(&self, turn_id: &str, run_id: StatefulRunId) {
        self.with_turn(turn_id, |turn| turn.run_id = Some(run_id));
    }

    pub(super) fn record_world_state(
        &self,
        turn_id: &str,
        root_entries_loaded: usize,
        audit: Option<&EvidenceAudit>,
        audit_recomputed: bool,
    ) {
        self.with_turn(turn_id, |turn| {
            turn.counters.world_state_samples += 1;
            turn.counters.root_entries_loaded += root_entries_loaded as u64;
            if audit_recomputed && let Some(audit) = audit {
                turn.counters.root_evidence_routes_checked += audit.statuses.len() as u64;
                turn.counters.root_evidence_routes_current += audit.current_count();
                turn.counters.root_evidence_routes_stale += audit.stale_count();
                turn.counters.root_evidence_routes_unavailable += audit.unavailable_count();
                turn.counters.root_evidence_routes_unchecked += audit.unchecked_count();
                turn.counters.root_unique_sources_observed += audit.observed_sources;
                turn.counters.root_source_bytes_hashed += audit.hashed_bytes;
            }
        });
    }

    fn prepare_material_findings(&self, turn_id: &str, call_id: &str, payload: &ToolPayload) {
        let ToolPayload::Function { arguments } = payload else {
            return;
        };
        let Ok(arguments) = serde_json::from_str::<RunUpdateAttributionArguments>(arguments) else {
            return;
        };
        if arguments.status.as_deref() != Some("completed") {
            return;
        }
        let count = arguments
            .material_root_findings
            .map_or(0, |items| items.len())
            + arguments
                .material_historical_findings
                .map_or(0, |items| items.len());
        self.with_turn(turn_id, |turn| {
            turn.pending_material_findings
                .insert(call_id.to_string(), count as u64);
        });
    }

    fn record_tool_outcome(
        &self,
        turn_id: &str,
        call_id: &str,
        tool_name: &str,
        outcome: ToolCallOutcome,
    ) {
        self.with_turn(turn_id, |turn| {
            turn.counters.stateful_tool_calls += 1;
            let successful = matches!(outcome, ToolCallOutcome::Completed { success: true });
            let material_findings = turn.pending_material_findings.remove(call_id).unwrap_or(0);
            if !successful {
                turn.counters.failed_stateful_tool_calls += 1;
                return;
            }
            turn.counters.material_findings_reused += material_findings;
            match tool_name {
                BLACKBOARD_QUERY | MEMORY_READ => turn.counters.knowledge_query_calls += 1,
                CONTEXT_MAP_QUERY => turn.counters.route_query_calls += 1,
                EVIDENCE_READ => turn.counters.evidence_read_calls += 1,
                STEERING_QUERY => turn.counters.steering_query_calls += 1,
                CONVERSATION_READ => turn.counters.conversation_read_calls += 1,
                BLACKBOARD_BATCH_RECORD | BLACKBOARD_UPDATE | BLACKBOARD_RELATE => {
                    turn.counters.blackboard_write_calls += 1
                }
                CONTEXT_MAP_REFRESH => turn.counters.context_refresh_calls += 1,
                OBLIGATION_UPDATE => turn.counters.obligation_write_calls += 1,
                STATEFUL_RUN_UPDATE => turn.counters.run_update_calls += 1,
                STEERING_RECONCILE => turn.counters.steering_write_calls += 1,
                _ => (),
            }
        });
    }

    pub(super) fn mark_failed(&self, turn_id: &str) {
        self.with_turn(turn_id, |turn| turn.failed = true);
    }

    pub(super) fn finish(
        &self,
        turn_id: &str,
        status: StatefulAttributionStatus,
    ) -> Option<StatefulAttributionSummary> {
        let turn = self
            .turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(turn_id)?;
        Some(StatefulAttributionSummary {
            run_id: turn.run_id,
            project_id: turn.project_id,
            thread_id: turn.thread_id,
            turn_id: turn_id.to_string(),
            status: if turn.failed {
                StatefulAttributionStatus::Failed
            } else {
                status
            },
            duration_ms: turn
                .started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            counters: turn.counters,
        })
    }

    fn with_turn(&self, turn_id: &str, update: impl FnOnce(&mut ActiveAttribution)) {
        if let Some(turn) = self
            .turns
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(turn_id)
        {
            update(turn);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunUpdateAttributionArguments {
    status: Option<String>,
    material_root_findings: Option<Vec<serde_json::Value>>,
    material_historical_findings: Option<Vec<serde_json::Value>>,
}

fn stateful_tool_name(name: &codex_extension_api::ToolName) -> Option<&str> {
    name.is_default_namespace()
        .then_some(name.name.as_str())
        .filter(|name| {
            matches!(
                *name,
                BLACKBOARD_QUERY
                    | CONTEXT_MAP_QUERY
                    | EVIDENCE_READ
                    | STEERING_QUERY
                    | CONVERSATION_READ
                    | MEMORY_READ
                    | BLACKBOARD_BATCH_RECORD
                    | BLACKBOARD_UPDATE
                    | BLACKBOARD_RELATE
                    | CONTEXT_MAP_REFRESH
                    | OBLIGATION_UPDATE
                    | STATEFUL_RUN_UPDATE
                    | STATEFUL_RUN_READ
                    | STEERING_RECONCILE
            )
        })
}

impl ToolLifecycleContributor for StatefulExtension {
    fn on_tool_start<'a>(&'a self, input: ToolStartInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            crate::window_journal::remember_plan_call(
                input.turn_store,
                input.tool_name,
                input.call_id,
                input.payload,
            );
            if stateful_tool_name(input.tool_name) == Some(STATEFUL_RUN_UPDATE) {
                self.attribution.prepare_material_findings(
                    input.turn_id,
                    input.call_id,
                    input.payload,
                );
            }
        })
    }

    fn on_tool_finish<'a>(&'a self, input: ToolFinishInput<'a>) -> ToolLifecycleFuture<'a> {
        Box::pin(async move {
            if let (Some(selected), Some(thread), Some(services)) = (
                input.thread_store.get::<SelectedProject>(),
                input.thread_store.get::<SelectedThread>(),
                self.services.as_ref(),
            ) && let Ok(store) = services.runtime().await
            {
                crate::window_journal::journal_plan_call(
                    store,
                    input.turn_store,
                    (selected.project_id(), &thread.thread_id, input.turn_id),
                    input.call_id,
                    input.outcome,
                )
                .await;
            }
            if let Some(thread) = input.thread_store.get::<SelectedThread>() {
                self.run_activity.for_thread(&thread.thread_id).record(
                    matches!(input.source, ToolCallSource::Direct),
                    stateful_tool_name(input.tool_name),
                    input.outcome,
                );
            }
            if let Some(tool_name) = stateful_tool_name(input.tool_name) {
                self.attribution.record_tool_outcome(
                    input.turn_id,
                    input.call_id,
                    tool_name,
                    input.outcome,
                );
            }
        })
    }
}

pub(super) fn begin_turn_attribution(
    extension: &StatefulExtension,
    turn_id: &str,
    thread_store: &ExtensionData,
) {
    let (Some(selected), Some(thread), Some(_services)) = (
        thread_store.get::<SelectedProject>(),
        thread_store.get::<SelectedThread>(),
        extension.services.as_ref(),
    ) else {
        return;
    };
    extension.attribution.begin(
        turn_id,
        selected.project_id().to_string(),
        thread.thread_id.clone(),
    );
}

pub(super) fn finish_turn_attribution(
    extension: &StatefulExtension,
    turn_store: &ExtensionData,
    status: StatefulAttributionStatus,
) {
    let turn_id = turn_store.level_id();
    let Some(summary) = extension.attribution.finish(turn_id, status) else {
        return;
    };
    if let Some(event_sink) = &extension.event_sink {
        event_sink.emit(StatefulEvent::AttributionCompleted { summary });
    }
}

pub(super) fn fail_turn_attribution(extension: &StatefulExtension, turn_id: &str) {
    extension.attribution.mark_failed(turn_id);
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
