use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

/// Bounded, content-free counters for one model turn.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct TurnTrajectory {
    #[ts(type = "number")]
    pub completed_model_responses: u64,
    #[ts(type = "number")]
    pub compactions: u64,
    #[ts(type = "number")]
    pub model_tool_calls: u64,
    #[ts(type = "number")]
    pub model_shell_tool_calls: u64,
    #[ts(type = "number")]
    pub model_function_tool_calls: u64,
    #[ts(type = "number")]
    pub model_custom_tool_calls: u64,
    #[ts(type = "number")]
    pub model_tool_search_calls: u64,
    #[ts(type = "number")]
    pub model_web_search_calls: u64,
    #[ts(type = "number")]
    pub model_image_generation_calls: u64,
    #[ts(type = "number")]
    /// Bytes in the JSON serialization of explicit tool output payloads.
    pub tool_output_bytes: u64,
}

/// Reports cumulative trajectory counters for the current turn without
/// exposing raw response items or tool payloads to clients.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct TurnTrajectoryUpdatedNotification {
    pub thread_id: String,
    pub turn_id: String,
    /// Whether this is the terminal snapshot for the turn. Transports may
    /// coalesce or drop intermediate updates, but must preserve this update.
    pub is_final: bool,
    pub trajectory: TurnTrajectory,
}
