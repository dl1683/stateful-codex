use std::sync::Arc;

use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use tempfile::TempDir;

use super::project_intelligence_tools;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

/// Serialized size of every Stateful tool (32,827 bytes before the 2026-10-02 trim). Verified
/// user-rule fields on the record tool took it to 18,223 while the direct roster fell to
/// 11,032; deferred tools are loaded only through tool search.
const MAX_ROSTER_BYTES: usize = 18_500;
/// Every request carries the directly exposed tools, so their size is a fixed per-request
/// cost; specialized tools are deferred to tool search.
const MAX_DIRECT_ROSTER_BYTES: usize = 11_500;

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
        crate::user_messages::UserMessageRegistry::default(),
    );
    let sizes = tools
        .iter()
        .map(|tool| {
            let bytes = serde_json::to_string(&tool.spec())
                .expect("spec serializes")
                .len();
            (
                tool.tool_name().to_string(),
                tool.exposure().is_direct(),
                bytes,
            )
        })
        .collect::<Vec<_>>();
    let total = sizes.iter().map(|(_, _, bytes)| bytes).sum::<usize>();
    let direct = sizes
        .iter()
        .filter(|(_, direct, _)| *direct)
        .map(|(_, _, bytes)| bytes)
        .sum::<usize>();

    assert!(
        total <= MAX_ROSTER_BYTES && direct <= MAX_DIRECT_ROSTER_BYTES,
        "Stateful roster is {total} bytes, {direct} direct: {sizes:?}"
    );
    assert!(
        tools
            .iter()
            .filter(|tool| tool.exposure().is_deferred())
            .all(|tool| tool.search_info().is_some()),
        "every deferred Stateful tool must be discoverable through tool search"
    );
}
