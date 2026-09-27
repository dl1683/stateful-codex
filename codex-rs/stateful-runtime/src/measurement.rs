use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;

use crate::StatefulRunId;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatefulTurnStatus {
    Completed,
    Failed,
    Aborted,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
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
    pub blackboard_write_calls: u64,
    pub context_refresh_calls: u64,
    pub obligation_write_calls: u64,
    pub run_update_calls: u64,
    pub steering_write_calls: u64,
    pub material_findings_reused: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TurnTrajectory {
    pub completed_model_responses: u64,
    pub compactions: u64,
    pub model_tool_calls: u64,
    pub model_shell_tool_calls: u64,
    pub model_function_tool_calls: u64,
    pub model_custom_tool_calls: u64,
    pub model_tool_search_calls: u64,
    pub model_web_search_calls: u64,
    pub model_image_generation_calls: u64,
    pub tool_output_bytes: u64,
}

/// Provider-reported token usage accumulated across completed model responses
/// in one turn.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StatefulTokenUsage {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulTurnTerminalMeasurement {
    pub status: StatefulTurnStatus,
    pub completed_at_ms: Option<i64>,
    pub trajectory: TurnTrajectory,
    /// `None` means the provider supplied no usage for any completed response.
    pub token_usage: Option<StatefulTokenUsage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewStatefulTurnMeasurement {
    pub run_id: StatefulRunId,
    pub project_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub status: StatefulTurnStatus,
    pub duration_ms: u64,
    pub attribution_counters: StatefulAttributionCounters,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulTurnMeasurement {
    pub value: NewStatefulTurnMeasurement,
    pub trajectory: Option<TurnTrajectory>,
    pub token_usage: Option<StatefulTokenUsage>,
    pub completed_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// One stable, project-scoped page of persisted Stateful turn measurements.
///
/// Callers must treat `next_cursor` as opaque and pass it back only with the
/// same project ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulTurnMeasurementPage {
    pub data: Vec<StatefulTurnMeasurement>,
    pub next_cursor: Option<String>,
}

/// Exact totals over a bounded newest-first window of project measurements.
///
/// This intentionally reports token usage rather than monetary cost: provider
/// pricing, units, and currency are not part of the persisted turn record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulMeasurementSummary {
    pub project_id: String,
    pub measurement_count: u64,
    pub run_count: u64,
    pub terminal_measurement_count: u64,
    pub completed_turns: u64,
    pub failed_turns: u64,
    pub aborted_turns: u64,
    pub turns_with_token_usage: u64,
    pub duration_ms: u64,
    pub attribution_counters: StatefulAttributionCounters,
    pub trajectory: Option<TurnTrajectory>,
    pub token_usage: Option<StatefulTokenUsage>,
    pub oldest_created_at_ms: Option<i64>,
    pub newest_created_at_ms: Option<i64>,
    pub has_more: bool,
}

impl StatefulMeasurementSummary {
    pub(crate) fn from_page(project_id: &str, page: StatefulTurnMeasurementPage) -> Self {
        let mut run_ids = HashSet::new();
        let mut summary = Self {
            project_id: project_id.to_string(),
            measurement_count: page.data.len() as u64,
            run_count: 0,
            terminal_measurement_count: 0,
            completed_turns: 0,
            failed_turns: 0,
            aborted_turns: 0,
            turns_with_token_usage: 0,
            duration_ms: 0,
            attribution_counters: StatefulAttributionCounters::default(),
            trajectory: None,
            token_usage: None,
            oldest_created_at_ms: None,
            newest_created_at_ms: None,
            has_more: page.next_cursor.is_some(),
        };

        for measurement in page.data {
            run_ids.insert(measurement.value.run_id.to_string());
            summary.duration_ms = summary
                .duration_ms
                .saturating_add(measurement.value.duration_ms);
            summary.oldest_created_at_ms = Some(
                summary
                    .oldest_created_at_ms
                    .map_or(measurement.created_at_ms, |value| {
                        value.min(measurement.created_at_ms)
                    }),
            );
            summary.newest_created_at_ms = Some(
                summary
                    .newest_created_at_ms
                    .map_or(measurement.created_at_ms, |value| {
                        value.max(measurement.created_at_ms)
                    }),
            );

            let counters = &measurement.value.attribution_counters;
            let total = &mut summary.attribution_counters;
            total.world_state_samples = total
                .world_state_samples
                .saturating_add(counters.world_state_samples);
            total.root_entries_loaded = total
                .root_entries_loaded
                .saturating_add(counters.root_entries_loaded);
            total.root_evidence_routes_checked = total
                .root_evidence_routes_checked
                .saturating_add(counters.root_evidence_routes_checked);
            total.root_evidence_routes_current = total
                .root_evidence_routes_current
                .saturating_add(counters.root_evidence_routes_current);
            total.root_evidence_routes_stale = total
                .root_evidence_routes_stale
                .saturating_add(counters.root_evidence_routes_stale);
            total.root_evidence_routes_unavailable = total
                .root_evidence_routes_unavailable
                .saturating_add(counters.root_evidence_routes_unavailable);
            total.root_evidence_routes_unchecked = total
                .root_evidence_routes_unchecked
                .saturating_add(counters.root_evidence_routes_unchecked);
            total.root_unique_sources_observed = total
                .root_unique_sources_observed
                .saturating_add(counters.root_unique_sources_observed);
            total.root_source_bytes_hashed = total
                .root_source_bytes_hashed
                .saturating_add(counters.root_source_bytes_hashed);
            total.stateful_tool_calls = total
                .stateful_tool_calls
                .saturating_add(counters.stateful_tool_calls);
            total.failed_stateful_tool_calls = total
                .failed_stateful_tool_calls
                .saturating_add(counters.failed_stateful_tool_calls);
            total.knowledge_query_calls = total
                .knowledge_query_calls
                .saturating_add(counters.knowledge_query_calls);
            total.route_query_calls = total
                .route_query_calls
                .saturating_add(counters.route_query_calls);
            total.evidence_read_calls = total
                .evidence_read_calls
                .saturating_add(counters.evidence_read_calls);
            total.steering_query_calls = total
                .steering_query_calls
                .saturating_add(counters.steering_query_calls);
            total.blackboard_write_calls = total
                .blackboard_write_calls
                .saturating_add(counters.blackboard_write_calls);
            total.context_refresh_calls = total
                .context_refresh_calls
                .saturating_add(counters.context_refresh_calls);
            total.obligation_write_calls = total
                .obligation_write_calls
                .saturating_add(counters.obligation_write_calls);
            total.run_update_calls = total
                .run_update_calls
                .saturating_add(counters.run_update_calls);
            total.steering_write_calls = total
                .steering_write_calls
                .saturating_add(counters.steering_write_calls);
            total.material_findings_reused = total
                .material_findings_reused
                .saturating_add(counters.material_findings_reused);

            if let Some(trajectory) = measurement.trajectory {
                summary.terminal_measurement_count =
                    summary.terminal_measurement_count.saturating_add(1);
                match measurement.value.status {
                    StatefulTurnStatus::Completed => {
                        summary.completed_turns = summary.completed_turns.saturating_add(1);
                    }
                    StatefulTurnStatus::Failed => {
                        summary.failed_turns = summary.failed_turns.saturating_add(1);
                    }
                    StatefulTurnStatus::Aborted => {
                        summary.aborted_turns = summary.aborted_turns.saturating_add(1);
                    }
                }
                let total = summary
                    .trajectory
                    .get_or_insert_with(TurnTrajectory::default);
                total.completed_model_responses = total
                    .completed_model_responses
                    .saturating_add(trajectory.completed_model_responses);
                total.compactions = total.compactions.saturating_add(trajectory.compactions);
                total.model_tool_calls = total
                    .model_tool_calls
                    .saturating_add(trajectory.model_tool_calls);
                total.model_shell_tool_calls = total
                    .model_shell_tool_calls
                    .saturating_add(trajectory.model_shell_tool_calls);
                total.model_function_tool_calls = total
                    .model_function_tool_calls
                    .saturating_add(trajectory.model_function_tool_calls);
                total.model_custom_tool_calls = total
                    .model_custom_tool_calls
                    .saturating_add(trajectory.model_custom_tool_calls);
                total.model_tool_search_calls = total
                    .model_tool_search_calls
                    .saturating_add(trajectory.model_tool_search_calls);
                total.model_web_search_calls = total
                    .model_web_search_calls
                    .saturating_add(trajectory.model_web_search_calls);
                total.model_image_generation_calls = total
                    .model_image_generation_calls
                    .saturating_add(trajectory.model_image_generation_calls);
                total.tool_output_bytes = total
                    .tool_output_bytes
                    .saturating_add(trajectory.tool_output_bytes);
            }

            if let Some(token_usage) = measurement.token_usage {
                summary.turns_with_token_usage = summary.turns_with_token_usage.saturating_add(1);
                let total = summary
                    .token_usage
                    .get_or_insert_with(StatefulTokenUsage::default);
                total.total_tokens = total.total_tokens.saturating_add(token_usage.total_tokens);
                total.input_tokens = total.input_tokens.saturating_add(token_usage.input_tokens);
                total.cached_input_tokens = total
                    .cached_input_tokens
                    .saturating_add(token_usage.cached_input_tokens);
                total.cache_write_input_tokens = total
                    .cache_write_input_tokens
                    .saturating_add(token_usage.cache_write_input_tokens);
                total.output_tokens = total
                    .output_tokens
                    .saturating_add(token_usage.output_tokens);
                total.reasoning_output_tokens = total
                    .reasoning_output_tokens
                    .saturating_add(token_usage.reasoning_output_tokens);
            }
        }
        summary.run_count = run_ids.len() as u64;
        summary
    }
}
