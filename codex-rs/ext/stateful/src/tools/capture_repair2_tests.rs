use super::capture_repair_tests::call;
use super::*;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolOutput;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn c3r2_decoded_proposal_refusals_and_backstops_fit_call_budgets() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let services = ProjectIntelligenceServices::new(sqlite.clone());
    let store = services.blackboard().await.unwrap();
    let before = store.project_revision("project-1").await.unwrap();
    let tool = blackboard_write::BlackboardBatchRecordTool::new(
        "project-1".into(),
        "00000000-0000-0000-0000-000000000001".into(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    let proposal = json!({"sourceId":"s".repeat(64),"digest":"a".repeat(64),
        "sourceRevision":1,"partIndex":0,"spans":[{"startByte":0,"endByte":1,"role":"body"}],
        "category":"background","interpretation":"Cedar equipment is teal."});
    for source in [
        ToolCallSource::Direct,
        ToolCallSource::CodeMode {
            cell_id: "cell".into(),
            runtime_tool_call_id: "nested".into(),
        },
    ] {
        for budget in [20, 32, 64, 100, 240, 9000] {
            for records in [
                vec![],
                vec![proposal.clone()],
                vec![proposal.clone(); 25],
                vec![{
                    let mut oversized = proposal.clone();
                    oversized["interpretation"] = json!("x".repeat(33000));
                    oversized
                }],
            ] {
                let call = call(
                    "blackboard_record_batch",
                    json!({"type":"sourceProposal","records":records}),
                    budget,
                    source.clone(),
                );
                let Err(FunctionCallError::RespondToModel(message)) =
                    tool.handle(call.clone()).await
                else {
                    panic!("refusal expected")
                };
                assert!(
                    serde_json::to_string(&message).unwrap().len()
                        <= call.response_byte_budget(MAX_RESPONSE_BYTES),
                    "{message}"
                );
                if budget == 20 && matches!(source, ToolCallSource::Direct) {
                    assert_eq!(message, "budget_insufficient");
                }
                let Err(FunctionCallError::RespondToModel(message)) =
                    bounded_json_output(&call, json!({"oversized":"x".repeat(10000)}))
                else {
                    panic!("backstop refusal")
                };
                assert!(
                    serde_json::to_string(&message).unwrap().len()
                        <= call.response_byte_budget(MAX_RESPONSE_BYTES)
                );
            }
        }
    }
    assert_eq!(store.project_revision("project-1").await.unwrap(), before);
}
