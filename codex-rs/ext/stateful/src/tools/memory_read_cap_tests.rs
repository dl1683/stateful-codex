use super::*;
use codex_extension_api::ToolCallSource;
use codex_project_intelligence::BlackboardEntryId;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::capture_repair_tests::call;

/// A truncation allowance far above every Stateful bound, so only the memory-read cap binds.
const GENEROUS: usize = 1_000_000;

/// Forty long user rules (the restored memory) overflow every memory read: each result stays
/// within the 8,192-byte memory-read bound, close to it, and says what it left out; an exact
/// read of escape-heavy words pages within the same bound.
#[tokio::test]
async fn memory_reads_stay_within_the_memory_read_bound() {
    let home = TempDir::new().unwrap();
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let node = services.project_node_id("project-1").await.unwrap();
    let store = services.blackboard().await.unwrap();
    for index in 0..40 {
        let mut rule = crate::tools::memory_read::tests::decision(
            &node,
            &format!(
                "Needle rule {index}: {}",
                "keep every reason. ".repeat(24).trim()
            ),
        );
        rule.provenance.kind = codex_project_intelligence::BlackboardProvenanceKind::User;
        store
            .create_entry(
                BlackboardEntryId::parse(format!("rule-{index:02}")).unwrap(),
                rule,
            )
            .await
            .unwrap();
    }
    let quotes = crate::tools::memory_read::tests::decision(&node, &"\"\\".repeat(2_000));
    store
        .create_entry(BlackboardEntryId::parse("quotes").unwrap(), quotes)
        .await
        .unwrap();
    let recall = memory_read::MemoryReadTool::new("project-1".into(), services.clone());
    let query = blackboard::BlackboardQueryTool::new(
        "project-1".into(),
        "00000000-0000-0000-0000-000000000001".into(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        VisibleRootRegistry::default(),
    );
    let outputs = [
        recall
            .handle(call(
                "memory_read",
                json!({"question": "needle"}),
                GENEROUS,
                ToolCallSource::Direct,
            ))
            .await
            .unwrap(),
        query
            .handle(call(
                "blackboard_query",
                json!({"text": "needle", "limit": 50, "detail": "full"}),
                GENEROUS,
                ToolCallSource::Direct,
            ))
            .await
            .unwrap(),
        query
            .handle(call(
                "blackboard_query",
                json!({"entryId": "quotes"}),
                GENEROUS,
                ToolCallSource::Direct,
            ))
            .await
            .unwrap(),
    ]
    .map(|output| serde_json::from_str::<serde_json::Value>(&output.log_output()).unwrap());
    for output in &outputs {
        let bytes = output.to_string().len();
        assert!(
            (6_000..=MAX_MEMORY_READ_BYTES).contains(&bytes),
            "{bytes} bytes"
        );
    }
    assert_eq!(
        (
            outputs[0]["coverage"]["entriesOmittedBySize"]
                .as_u64()
                .unwrap()
                > 0,
            outputs[1]["truncated"].clone(),
            outputs[2]["complete"].clone(),
        ),
        (true, json!(true), json!(false))
    );
}
