use codex_app_server_protocol::TurnTrajectory;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_stateful_runtime::StatefulTokenUsage;
use std::collections::HashSet;
use std::io::Write;

const MAX_TRACKED_TOOL_CALL_IDS: usize = 1_024;
const MAX_TRACKED_TOOL_CALL_ID_BYTES: usize = 256;

#[derive(Default)]
pub(crate) struct TurnTrajectoryState {
    turn_id: Option<String>,
    trajectory: TurnTrajectory,
    token_usage: Option<StatefulTokenUsage>,
    observed_tool_call_ids: HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TurnTrajectorySnapshot {
    pub(crate) trajectory: TurnTrajectory,
    pub(crate) token_usage: Option<StatefulTokenUsage>,
}

impl TurnTrajectoryState {
    pub(crate) fn observe(
        &mut self,
        event_turn_id: &str,
        event: &EventMsg,
    ) -> Option<TurnTrajectorySnapshot> {
        if matches!(event, EventMsg::TurnStarted(_)) {
            self.turn_id = Some(event_turn_id.to_string());
            self.trajectory = TurnTrajectory::default();
            self.token_usage = None;
            self.observed_tool_call_ids.clear();
            return None;
        }
        if !matches!(
            event,
            EventMsg::RawResponseItem(_)
                | EventMsg::RawResponseCompleted(_)
                | EventMsg::ContextCompacted(_)
                | EventMsg::TurnComplete(_)
                | EventMsg::TurnAborted(_)
        ) {
            return None;
        }
        if self.turn_id.is_none() {
            self.turn_id = Some(event_turn_id.to_string());
            self.trajectory = TurnTrajectory::default();
            self.token_usage = None;
            self.observed_tool_call_ids.clear();
        } else if self.turn_id.as_deref() != Some(event_turn_id) {
            return None;
        }

        match event {
            EventMsg::RawResponseItem(event) => {
                self.record_response_item(&event.item);
                None
            }
            EventMsg::RawResponseCompleted(event) => {
                self.trajectory.completed_model_responses =
                    self.trajectory.completed_model_responses.saturating_add(1);
                if let Some(usage) = event.token_usage.as_ref() {
                    let accumulated = self.token_usage.get_or_insert_default();
                    accumulated.total_tokens =
                        accumulated.total_tokens.saturating_add(usage.total_tokens);
                    accumulated.input_tokens =
                        accumulated.input_tokens.saturating_add(usage.input_tokens);
                    accumulated.cached_input_tokens = accumulated
                        .cached_input_tokens
                        .saturating_add(usage.cached_input_tokens);
                    accumulated.cache_write_input_tokens = accumulated
                        .cache_write_input_tokens
                        .saturating_add(usage.cache_write_input_tokens);
                    accumulated.output_tokens = accumulated
                        .output_tokens
                        .saturating_add(usage.output_tokens);
                    accumulated.reasoning_output_tokens = accumulated
                        .reasoning_output_tokens
                        .saturating_add(usage.reasoning_output_tokens);
                }
                Some(self.snapshot())
            }
            EventMsg::ContextCompacted(_) => {
                self.trajectory.compactions = self.trajectory.compactions.saturating_add(1);
                Some(self.snapshot())
            }
            EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => Some(self.snapshot()),
            _ => None,
        }
    }

    fn snapshot(&self) -> TurnTrajectorySnapshot {
        TurnTrajectorySnapshot {
            trajectory: self.trajectory.clone(),
            token_usage: self.token_usage.clone(),
        }
    }

    fn record_response_item(&mut self, item: &ResponseItem) {
        match item {
            ResponseItem::LocalShellCall { call_id, .. } => {
                self.trajectory.model_tool_calls =
                    self.trajectory.model_tool_calls.saturating_add(1);
                self.trajectory.model_shell_tool_calls =
                    self.trajectory.model_shell_tool_calls.saturating_add(1);
                self.record_call_id(call_id.as_deref());
            }
            ResponseItem::FunctionCall { call_id, .. } => {
                self.trajectory.model_tool_calls =
                    self.trajectory.model_tool_calls.saturating_add(1);
                self.trajectory.model_function_tool_calls =
                    self.trajectory.model_function_tool_calls.saturating_add(1);
                self.record_call_id(Some(call_id));
            }
            ResponseItem::CustomToolCall { call_id, .. } => {
                self.trajectory.model_tool_calls =
                    self.trajectory.model_tool_calls.saturating_add(1);
                self.trajectory.model_custom_tool_calls =
                    self.trajectory.model_custom_tool_calls.saturating_add(1);
                self.record_call_id(Some(call_id));
            }
            ResponseItem::ToolSearchCall { call_id, .. } => {
                self.trajectory.model_tool_calls =
                    self.trajectory.model_tool_calls.saturating_add(1);
                self.trajectory.model_tool_search_calls =
                    self.trajectory.model_tool_search_calls.saturating_add(1);
                self.record_call_id(call_id.as_deref());
            }
            ResponseItem::WebSearchCall { .. } => {
                self.trajectory.model_tool_calls =
                    self.trajectory.model_tool_calls.saturating_add(1);
                self.trajectory.model_web_search_calls =
                    self.trajectory.model_web_search_calls.saturating_add(1);
            }
            ResponseItem::ImageGenerationCall { result, .. } => {
                self.trajectory.model_tool_calls =
                    self.trajectory.model_tool_calls.saturating_add(1);
                self.trajectory.model_image_generation_calls = self
                    .trajectory
                    .model_image_generation_calls
                    .saturating_add(1);
                self.trajectory.tool_output_bytes = self
                    .trajectory
                    .tool_output_bytes
                    .saturating_add(serialized_len(result));
            }
            ResponseItem::FunctionCallOutput {
                call_id: Some(call_id),
                output,
                ..
            }
            | ResponseItem::CustomToolCallOutput {
                call_id, output, ..
            } if self.observed_tool_call_ids.remove(call_id) => {
                self.trajectory.tool_output_bytes = self
                    .trajectory
                    .tool_output_bytes
                    .saturating_add(serialized_len(output));
            }
            ResponseItem::ToolSearchOutput {
                call_id: Some(call_id),
                tools,
                ..
            } if self.observed_tool_call_ids.remove(call_id) => {
                self.trajectory.tool_output_bytes = self
                    .trajectory
                    .tool_output_bytes
                    .saturating_add(serialized_len(tools));
            }
            ResponseItem::AdditionalTools { .. }
            | ResponseItem::Message { .. }
            | ResponseItem::AgentMessage { .. }
            | ResponseItem::Reasoning { .. }
            | ResponseItem::FunctionCallOutput { .. }
            | ResponseItem::CustomToolCallOutput { .. }
            | ResponseItem::ToolSearchOutput { .. }
            | ResponseItem::Compaction { .. }
            | ResponseItem::ConfigurationUpdate { .. }
            | ResponseItem::CompactionTrigger {}
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => {}
        }
    }

    fn record_call_id(&mut self, call_id: Option<&str>) {
        let Some(call_id) = call_id else {
            return;
        };
        if call_id.len() <= MAX_TRACKED_TOOL_CALL_ID_BYTES
            && self.observed_tool_call_ids.len() < MAX_TRACKED_TOOL_CALL_IDS
        {
            self.observed_tool_call_ids.insert(call_id.to_string());
        }
    }
}

fn serialized_len(value: &impl serde::Serialize) -> u64 {
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, value)
        .map(|()| writer.bytes)
        .unwrap_or_default()
}

#[derive(Default)]
struct CountingWriter {
    bytes: u64,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len() as u64);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "turn_trajectory_tests.rs"]
mod tests;
