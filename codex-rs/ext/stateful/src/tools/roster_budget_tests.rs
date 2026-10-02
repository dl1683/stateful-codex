use std::sync::Arc;

use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use tempfile::TempDir;

use super::project_intelligence_tools;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

/// Every request carries the whole Stateful roster, so its serialized size is a fixed
/// per-request cost (32,827 bytes before the 2026-10-02 trim).
const MAX_ROSTER_BYTES: usize = 18_000;

#[test]
fn stateful_tool_roster_stays_within_its_request_budget() {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let tools = project_intelligence_tools(
        "project-1".to_string(),
        "thread-1".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    let sizes = tools
        .iter()
        .map(|tool| {
            let bytes = serde_json::to_string(&tool.spec())
                .expect("spec serializes")
                .len();
            (tool.tool_name().to_string(), bytes)
        })
        .collect::<Vec<_>>();
    let total = sizes.iter().map(|(_, bytes)| bytes).sum::<usize>();

    assert!(
        total <= MAX_ROSTER_BYTES,
        "Stateful roster is {total} bytes: {sizes:?}"
    );
}
