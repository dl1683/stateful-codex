use std::time::Instant;

use codex_app_server_protocol::StatefulAttributionCompletedNotification;
use codex_app_server_protocol::StatefulAttributionStatus;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TurnTrajectory;
use codex_app_server_protocol::TurnTrajectoryUpdatedNotification;

use crate::exec_events::RunTrajectory;
use crate::exec_events::StatefulAttribution;
use crate::exec_events::TurnProgressEvent;
use crate::exec_events::Usage;

#[derive(Debug, Default)]
pub(crate) struct StatefulAttributionAccumulator {
    stateful: Option<StatefulAttribution>,
    invocation_started_at: Option<Instant>,
    completed_trajectory: TurnTrajectory,
    current_turn_id: Option<String>,
    current_turn_trajectory: TurnTrajectory,
    active_usage_turn_id: Option<String>,
    usage_baseline: Usage,
    latest_total_usage: Option<Usage>,
    current_usage_seen: bool,
}

impl StatefulAttributionAccumulator {
    pub(crate) fn start_turn(&mut self, turn_id: &str) {
        self.invocation_started_at.get_or_insert_with(Instant::now);
        self.active_usage_turn_id = Some(turn_id.to_string());
    }

    pub(crate) fn record_token_usage(
        &mut self,
        notification: &ThreadTokenUsageUpdatedNotification,
    ) -> bool {
        let total = usage_from_breakdown(&notification.token_usage.total);
        if self.active_usage_turn_id.as_deref() != Some(notification.turn_id.as_str()) {
            if !self.current_usage_seen {
                self.usage_baseline = total;
            }
            return false;
        }
        let changed = self
            .latest_total_usage
            .as_ref()
            .is_none_or(|previous| previous != &total);
        self.latest_total_usage = Some(total);
        self.current_usage_seen = true;
        changed
    }

    pub(crate) fn usage(&self) -> Usage {
        self.latest_total_usage
            .as_ref()
            .map_or_else(Usage::default, |total| Usage {
                input_tokens: non_negative_delta(
                    total.input_tokens,
                    self.usage_baseline.input_tokens,
                ),
                cached_input_tokens: non_negative_delta(
                    total.cached_input_tokens,
                    self.usage_baseline.cached_input_tokens,
                ),
                cache_write_input_tokens: non_negative_delta(
                    total.cache_write_input_tokens,
                    self.usage_baseline.cache_write_input_tokens,
                ),
                output_tokens: non_negative_delta(
                    total.output_tokens,
                    self.usage_baseline.output_tokens,
                ),
                reasoning_output_tokens: non_negative_delta(
                    total.reasoning_output_tokens,
                    self.usage_baseline.reasoning_output_tokens,
                ),
            })
    }

    pub(crate) fn record_trajectory(
        &mut self,
        notification: &TurnTrajectoryUpdatedNotification,
    ) -> bool {
        if self.current_turn_id.as_deref() != Some(notification.turn_id.as_str()) {
            add_trajectory(
                &mut self.completed_trajectory,
                &self.current_turn_trajectory,
            );
            self.current_turn_id = Some(notification.turn_id.clone());
            self.current_turn_trajectory = TurnTrajectory::default();
        }
        let mut merged = self.current_turn_trajectory.clone();
        merge_trajectory_max(&mut merged, &notification.trajectory);
        if self.current_turn_trajectory == merged {
            return false;
        }
        self.current_turn_trajectory = merged;
        true
    }

    pub(crate) fn record_stateful_turn(
        &mut self,
        notification: &StatefulAttributionCompletedNotification,
    ) {
        let attribution = self
            .stateful
            .get_or_insert_with(StatefulAttribution::default);
        attribution.turns += 1;
        match notification.status {
            StatefulAttributionStatus::Completed => attribution.completed_turns += 1,
            StatefulAttributionStatus::Failed => attribution.failed_turns += 1,
            StatefulAttributionStatus::Aborted => attribution.aborted_turns += 1,
        }
        attribution.duration_ms += notification.duration_ms;
        let counters = &notification.counters;
        attribution.world_state_samples += counters.world_state_samples;
        attribution.root_entries_loaded += counters.root_entries_loaded;
        attribution.root_evidence_routes_checked += counters.root_evidence_routes_checked;
        attribution.root_evidence_routes_current += counters.root_evidence_routes_current;
        attribution.root_evidence_routes_stale += counters.root_evidence_routes_stale;
        attribution.root_evidence_routes_unavailable += counters.root_evidence_routes_unavailable;
        attribution.root_evidence_routes_unchecked += counters.root_evidence_routes_unchecked;
        attribution.root_unique_sources_observed += counters.root_unique_sources_observed;
        attribution.root_source_bytes_hashed += counters.root_source_bytes_hashed;
        attribution.stateful_tool_calls += counters.stateful_tool_calls;
        attribution.failed_stateful_tool_calls += counters.failed_stateful_tool_calls;
        attribution.knowledge_query_calls += counters.knowledge_query_calls;
        attribution.route_query_calls += counters.route_query_calls;
        attribution.evidence_read_calls += counters.evidence_read_calls;
        attribution.steering_query_calls += counters.steering_query_calls;
        attribution.conversation_read_calls += counters.conversation_read_calls;
        attribution.blackboard_write_calls += counters.blackboard_write_calls;
        attribution.context_refresh_calls += counters.context_refresh_calls;
        attribution.obligation_write_calls += counters.obligation_write_calls;
        attribution.run_update_calls += counters.run_update_calls;
        attribution.steering_write_calls += counters.steering_write_calls;
        attribution.material_findings_reused += counters.material_findings_reused;
    }

    pub(crate) fn snapshot(&self) -> Option<StatefulAttribution> {
        let mut attribution = self.stateful.clone()?;
        let trajectory = self.trajectory();
        attribution.invocation_duration_ms = trajectory.invocation_duration_ms;
        attribution.completed_model_responses = trajectory.completed_model_responses;
        attribution.compactions = trajectory.compactions;
        attribution.model_tool_calls = trajectory.model_tool_calls;
        attribution.model_shell_tool_calls = trajectory.model_shell_tool_calls;
        attribution.model_function_tool_calls = trajectory.model_function_tool_calls;
        attribution.model_custom_tool_calls = trajectory.model_custom_tool_calls;
        attribution.model_tool_search_calls = trajectory.model_tool_search_calls;
        attribution.model_web_search_calls = trajectory.model_web_search_calls;
        attribution.model_image_generation_calls = trajectory.model_image_generation_calls;
        attribution.tool_output_bytes = trajectory.tool_output_bytes;
        Some(attribution)
    }

    pub(crate) fn progress(&self, usage: Usage) -> TurnProgressEvent {
        let trajectory = self.trajectory();
        TurnProgressEvent {
            usage,
            elapsed_ms: trajectory.invocation_duration_ms,
            completed_model_responses: trajectory.completed_model_responses,
            compactions: trajectory.compactions,
            model_tool_calls: trajectory.model_tool_calls,
            tool_output_bytes: trajectory.tool_output_bytes,
            trajectory,
        }
    }

    pub(crate) fn trajectory(&self) -> RunTrajectory {
        let mut trajectory = self.completed_trajectory.clone();
        add_trajectory(&mut trajectory, &self.current_turn_trajectory);
        RunTrajectory {
            invocation_duration_ms: self
                .invocation_started_at
                .map(|started_at| started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
                .unwrap_or_default(),
            completed_model_responses: trajectory.completed_model_responses,
            compactions: trajectory.compactions,
            model_tool_calls: trajectory.model_tool_calls,
            model_shell_tool_calls: trajectory.model_shell_tool_calls,
            model_function_tool_calls: trajectory.model_function_tool_calls,
            model_custom_tool_calls: trajectory.model_custom_tool_calls,
            model_tool_search_calls: trajectory.model_tool_search_calls,
            model_web_search_calls: trajectory.model_web_search_calls,
            model_image_generation_calls: trajectory.model_image_generation_calls,
            tool_output_bytes: trajectory.tool_output_bytes,
        }
    }
}

fn usage_from_breakdown(usage: &codex_app_server_protocol::TokenUsageBreakdown) -> Usage {
    Usage {
        input_tokens: usage.input_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        cache_write_input_tokens: usage.cache_write_input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_output_tokens: usage.reasoning_output_tokens,
    }
}

fn non_negative_delta(total: i64, baseline: i64) -> i64 {
    total.saturating_sub(baseline).max(0)
}

fn add_trajectory(total: &mut TurnTrajectory, value: &TurnTrajectory) {
    total.completed_model_responses = total
        .completed_model_responses
        .saturating_add(value.completed_model_responses);
    total.compactions = total.compactions.saturating_add(value.compactions);
    total.model_tool_calls = total
        .model_tool_calls
        .saturating_add(value.model_tool_calls);
    total.model_shell_tool_calls = total
        .model_shell_tool_calls
        .saturating_add(value.model_shell_tool_calls);
    total.model_function_tool_calls = total
        .model_function_tool_calls
        .saturating_add(value.model_function_tool_calls);
    total.model_custom_tool_calls = total
        .model_custom_tool_calls
        .saturating_add(value.model_custom_tool_calls);
    total.model_tool_search_calls = total
        .model_tool_search_calls
        .saturating_add(value.model_tool_search_calls);
    total.model_web_search_calls = total
        .model_web_search_calls
        .saturating_add(value.model_web_search_calls);
    total.model_image_generation_calls = total
        .model_image_generation_calls
        .saturating_add(value.model_image_generation_calls);
    total.tool_output_bytes = total
        .tool_output_bytes
        .saturating_add(value.tool_output_bytes);
}

fn merge_trajectory_max(current: &mut TurnTrajectory, update: &TurnTrajectory) {
    current.completed_model_responses = current
        .completed_model_responses
        .max(update.completed_model_responses);
    current.compactions = current.compactions.max(update.compactions);
    current.model_tool_calls = current.model_tool_calls.max(update.model_tool_calls);
    current.model_shell_tool_calls = current
        .model_shell_tool_calls
        .max(update.model_shell_tool_calls);
    current.model_function_tool_calls = current
        .model_function_tool_calls
        .max(update.model_function_tool_calls);
    current.model_custom_tool_calls = current
        .model_custom_tool_calls
        .max(update.model_custom_tool_calls);
    current.model_tool_search_calls = current
        .model_tool_search_calls
        .max(update.model_tool_search_calls);
    current.model_web_search_calls = current
        .model_web_search_calls
        .max(update.model_web_search_calls);
    current.model_image_generation_calls = current
        .model_image_generation_calls
        .max(update.model_image_generation_calls);
    current.tool_output_bytes = current.tool_output_bytes.max(update.tool_output_bytes);
}
