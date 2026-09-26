use std::time::Instant;

use codex_app_server_protocol::StatefulAttributionCompletedNotification;
use codex_app_server_protocol::StatefulAttributionStatus;
use codex_protocol::models::ResponseItem;

use crate::exec_events::StatefulAttribution;

#[derive(Debug, Default)]
pub(crate) struct StatefulAttributionAccumulator {
    stateful: Option<StatefulAttribution>,
    invocation_started_at: Option<Instant>,
    completed_model_responses: u64,
    compactions: u64,
    model_tool_calls: u64,
    model_shell_tool_calls: u64,
    model_function_tool_calls: u64,
    model_custom_tool_calls: u64,
    model_tool_search_calls: u64,
    model_web_search_calls: u64,
    model_image_generation_calls: u64,
    tool_output_bytes: u64,
}

impl StatefulAttributionAccumulator {
    pub(crate) fn start_invocation(&mut self) {
        self.invocation_started_at.get_or_insert_with(Instant::now);
    }

    pub(crate) fn record_model_response(&mut self) {
        self.completed_model_responses += 1;
    }

    pub(crate) fn record_compaction(&mut self) {
        self.compactions += 1;
    }

    pub(crate) fn record_response_item(&mut self, item: &ResponseItem) {
        match item {
            ResponseItem::LocalShellCall { .. } => {
                self.model_tool_calls += 1;
                self.model_shell_tool_calls += 1;
            }
            ResponseItem::FunctionCall { .. } => {
                self.model_tool_calls += 1;
                self.model_function_tool_calls += 1;
            }
            ResponseItem::CustomToolCall { .. } => {
                self.model_tool_calls += 1;
                self.model_custom_tool_calls += 1;
            }
            ResponseItem::ToolSearchCall { .. } => {
                self.model_tool_calls += 1;
                self.model_tool_search_calls += 1;
            }
            ResponseItem::WebSearchCall { .. } => {
                self.model_tool_calls += 1;
                self.model_web_search_calls += 1;
            }
            ResponseItem::ImageGenerationCall { result, .. } => {
                self.model_tool_calls += 1;
                self.model_image_generation_calls += 1;
                self.tool_output_bytes += result.len() as u64;
            }
            ResponseItem::FunctionCallOutput { output, .. }
            | ResponseItem::CustomToolCallOutput { output, .. } => {
                self.tool_output_bytes += serialized_len(output);
            }
            ResponseItem::ToolSearchOutput { tools, .. } => {
                self.tool_output_bytes += serialized_len(tools);
            }
            ResponseItem::AdditionalTools { .. }
            | ResponseItem::Message { .. }
            | ResponseItem::AgentMessage { .. }
            | ResponseItem::Reasoning { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::ConfigurationUpdate { .. }
            | ResponseItem::CompactionTrigger {}
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => {}
        }
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
        attribution.blackboard_write_calls += counters.blackboard_write_calls;
        attribution.context_refresh_calls += counters.context_refresh_calls;
        attribution.obligation_write_calls += counters.obligation_write_calls;
        attribution.run_update_calls += counters.run_update_calls;
        attribution.steering_write_calls += counters.steering_write_calls;
        attribution.material_findings_reused += counters.material_findings_reused;
    }

    pub(crate) fn snapshot(&self) -> Option<StatefulAttribution> {
        let mut attribution = self.stateful.clone()?;
        attribution.invocation_duration_ms = self
            .invocation_started_at
            .map(|started_at| started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or_default();
        attribution.completed_model_responses = self.completed_model_responses;
        attribution.compactions = self.compactions;
        attribution.model_tool_calls = self.model_tool_calls;
        attribution.model_shell_tool_calls = self.model_shell_tool_calls;
        attribution.model_function_tool_calls = self.model_function_tool_calls;
        attribution.model_custom_tool_calls = self.model_custom_tool_calls;
        attribution.model_tool_search_calls = self.model_tool_search_calls;
        attribution.model_web_search_calls = self.model_web_search_calls;
        attribution.model_image_generation_calls = self.model_image_generation_calls;
        attribution.tool_output_bytes = self.tool_output_bytes;
        Some(attribution)
    }
}

fn serialized_len(value: &impl serde::Serialize) -> u64 {
    serde_json::to_vec(value)
        .map(|value| value.len() as u64)
        .unwrap_or_default()
}
