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
    let proposal = json!({"sourceId":"s".repeat(/*n*/ 64),"digest":"a".repeat(/*n*/ 64),
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
                    oversized["interpretation"] = json!("x".repeat(/*n*/ 33000));
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
                    bounded_json_output(&call, json!({"oversized":"x".repeat(/*n*/ 10000)}))
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

#[tokio::test]
async fn c3r2_memory_read_omitted_matches_refuse_without_progress_cold() {
    let home = TempDir::new().unwrap();
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let services = ProjectIntelligenceServices::new(sqlite.clone());
    let write = blackboard_write::BlackboardBatchRecordTool::new(
        "project-1".into(),
        "thread-1".into(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    write.handle(call("blackboard_record_batch",json!({"records":[{
        "idempotencyKey":"budget-entry","kind":"note","content":format!("equipment {}", "a".repeat(/*n*/ 3990)),
        "confidenceBasisPoints":0,"verification":"unverified","importance":"normal","rootPromotion":"notPromoted"
    }]}),/*budget*/ 9000,ToolCallSource::Direct)).await.unwrap();
    for _ in 0..2 {
        let services = ProjectIntelligenceServices::new(sqlite.clone());
        let store = services.blackboard().await.unwrap();
        let before = store.project_revision("project-1").await.unwrap();
        let tool = memory_read::MemoryReadTool::new("project-1".into(), services.clone());
        let mut refused = false;
        let mut delivered = false;
        for budget in [20, 100, 800, 1200, 1600, 2000, 4000, 9000] {
            let call = call(
                "memory_read",
                json!({"question":"equipment","includeHistory":false}),
                budget,
                ToolCallSource::Direct,
            );
            match tool.handle(call.clone()).await {
                Err(FunctionCallError::RespondToModel(message)) => {
                    assert_eq!(message, "budget_insufficient");
                    assert!(
                        serde_json::to_string(&message).unwrap().len()
                            <= call.response_byte_budget(MAX_RESPONSE_BYTES)
                    );
                    refused = true;
                }
                Ok(output) => {
                    let result: serde_json::Value =
                        serde_json::from_str(&output.log_output()).unwrap();
                    assert_eq!(result["entries"].as_array().unwrap().len(), 1);
                    assert!(
                        output.log_output().len() <= call.response_byte_budget(MAX_RESPONSE_BYTES)
                    );
                    delivered = true;
                }
                Err(error) => panic!("{error}"),
            }
        }
        assert!(refused && delivered);
        let output = tool
            .handle(call(
                "memory_read",
                json!({"question":"absent"}),
                /*budget*/ 9000,
                ToolCallSource::Direct,
            ))
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&output.log_output()).unwrap();
        assert_eq!(result["entries"], json!([]));
        assert_eq!(store.project_revision("project-1").await.unwrap(), before);
    }
}
