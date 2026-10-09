use codex_extension_api::ToolName;

use super::tool_has_effects;

#[test]
fn only_the_hosts_reading_and_bookkeeping_tools_have_no_effects() {
    for name in [
        "update_plan",
        "view_image",
        "stateful_run_update",
        "obligation_update",
        "steering_reconcile",
        "blackboard_query",
    ] {
        assert!(!tool_has_effects(&ToolName::plain(name)), "{name}");
    }
    for name in [
        "exec_command",
        "shell",
        "apply_patch",
        "write_stdin",
        "spawn_agent",
        "exec",
        "unknown_tool",
        "blackboard_record_batch",
        "blackboard_update_batch",
        "blackboard_relate",
        "context_map_refresh",
        "read_mcp_resource",
    ] {
        assert!(tool_has_effects(&ToolName::plain(name)), "{name}");
    }
    assert!(tool_has_effects(&ToolName::namespaced(
        "mcp__files",
        "read"
    )));
}
