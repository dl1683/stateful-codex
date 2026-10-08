use std::sync::Arc;

use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_project_intelligence::BlackboardEntryId;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::WorkflowMode;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;

use super::blackboard_write::BlackboardBatchRecordTool;
use super::capture_repair_tests::call;
use super::obligation::ObligationUpdateTool;
use super::provenance_source_id;
use super::stable_id;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

const PROJECT_ID: &str = "project-1";
const THREAD_ID: &str = "thread-1";

/// A Responses bridge call ID that carries a Gemini thought signature, as LiteLLM emits it.
fn bridged_call_id() -> String {
    format!(
        "call_3f1c9a7e2b__thought__{}",
        "Q2lZQjQ0a2Z".repeat(/*n*/ 200)
    )
}

fn digest_source(call_id: &str) -> String {
    format!("call-sha256:{:x}", Sha256::digest(call_id.as_bytes()))
}

#[test]
fn provenance_keeps_short_call_ids_and_digests_unbounded_ones() {
    let bridged = bridged_call_id();
    assert_eq!(
        [
            provenance_source_id("call_abc123"),
            provenance_source_id(&bridged),
            provenance_source_id(" call_padded"),
        ],
        [
            "call_abc123".to_string(),
            digest_source(&bridged),
            digest_source(" call_padded"),
        ]
    );
}

#[tokio::test]
async fn bridged_call_ids_record_findings_and_obligations() {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let run = services
        .runtime()
        .await
        .expect("runtime opens")
        .create_run(
            StatefulRunId::parse("run-0123456789").expect("valid run ID"),
            NewStatefulRun {
                project_id: PROJECT_ID.to_string(),
                thread_ids: vec![THREAD_ID.to_string()],
                goal: "Fix the calculator.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run is created");
    let bridged = bridged_call_id();

    let mut record = call(
        "blackboard_record_batch",
        json!({"records": [{
            "idempotencyKey": "finding-1",
            "kind": "fact",
            "content": "calc.add subtracted its operands.",
            "confidenceBasisPoints": 9000,
            "verification": "unverified",
            "importance": "normal",
            "rootPromotion": "notPromoted"
        }]}),
        /*budget*/ 20_000,
        ToolCallSource::Direct,
    );
    record.call_id = bridged.clone();
    BlackboardBatchRecordTool::new(
        PROJECT_ID.to_string(),
        THREAD_ID.to_string(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    )
    .handle(record)
    .await
    .expect("the batch is accepted");
    let entry = services
        .blackboard()
        .await
        .expect("blackboard opens")
        .get_entry(
            PROJECT_ID,
            &BlackboardEntryId::parse(stable_id("entry", PROJECT_ID, "finding-1"))
                .expect("valid entry ID"),
        )
        .await
        .expect("entry reads")
        .expect("the finding was recorded");

    let mut update = call(
        "obligation_update",
        json!({
            "idempotencyKey": "progress-1",
            "packet": {"learning": ["calc.add subtracted"], "next": ["fix calc.add"]}
        }),
        /*budget*/ 20_000,
        ToolCallSource::Direct,
    );
    update.call_id = bridged.clone();
    ObligationUpdateTool::new(
        PROJECT_ID.to_string(),
        THREAD_ID.to_string(),
        services.clone(),
        /*event_sink*/ None,
    )
    .handle(update)
    .await
    .expect("the obligation is accepted");
    let obligations = services
        .runtime()
        .await
        .expect("runtime opens")
        .list_obligations(
            &run.id, /*after_sequence*/ None, /*max_results*/ 10,
        )
        .await
        .expect("obligations list");

    assert_eq!(
        (
            entry.value.provenance.source_id,
            obligations
                .into_iter()
                .map(|obligation| obligation.value.provenance_source_id)
                .collect::<Vec<_>>(),
        ),
        (digest_source(&bridged), vec![digest_source(&bridged)])
    );
}
