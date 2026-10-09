//! Host classification of actions for the read-only exemption. The host cannot attest what a
//! command executes (program resolution, shell startup, functions and environment are outside
//! its control, and no per-command read-only sandbox policy is recorded), so every executed
//! command is an effect: only runs without commands can be exempt. A tool call is effect-free
//! only when it is one of the host's own reading or run-bookkeeping tools; every other tool,
//! unknown, namespaced and command tools included, is an effect.
//!
//! An effect-capable tool's effect is recorded durably as an intent before the tool is allowed
//! to run (see `record_effect_intent`); if the intent cannot be recorded the call is blocked,
//! so no tool action can go unaccounted.

use codex_extension_api::ExtensionData;
use codex_extension_api::ToolName;

use crate::acceptance_observation::TurnRunBinding;
use crate::services::ProjectIntelligenceServices;

/// The host's own tools that only read, or that keep the run's own bookkeeping (its
/// obligations, steering and acceptance records). Project-memory writers, index refreshes and
/// MCP calls are effects.
const READ_ONLY_TOOLS: &[&str] = &[
    "blackboard_query",
    "context_map_query",
    "conversation_read",
    "evidence_read",
    "memory_read",
    "obligation_update",
    "request_user_input",
    "stateful_acceptance_update",
    "stateful_run_read",
    "stateful_run_update",
    "steering_query",
    "steering_reconcile",
    "tool_search",
    "update_plan",
    "view_image",
];

/// Whether a tool call may have effects outside the run's own bookkeeping.
pub(crate) fn tool_has_effects(tool_name: &ToolName) -> bool {
    !(tool_name.is_default_namespace() && READ_ONLY_TOOLS.contains(&tool_name.name.as_str()))
}

/// Before an effect-capable tool runs, durably records its (possible) effect against the run
/// of the turn. An error means the intent is not durable, and the caller must block the call.
pub(crate) async fn record_effect_intent(
    services: &ProjectIntelligenceServices,
    turn_store: &ExtensionData,
    tool_name: &ToolName,
) -> Result<(), String> {
    if !tool_has_effects(tool_name) {
        return Ok(());
    }
    let Some(binding) = turn_store.get::<TurnRunBinding>() else {
        return Ok(());
    };
    services
        .runtime()
        .await
        .map_err(|error| error.to_string())?
        .record_side_effect(&binding.run_id)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "acceptance_effects_tests.rs"]
mod tests;
