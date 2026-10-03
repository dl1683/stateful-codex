use std::sync::Arc;

use codex_extension_api::ConversationHistory;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::BlackboardBatchRecordTool;
use crate::services::ProjectIntelligenceServices;
use crate::user_messages::UserMessageRegistry;
use crate::visible_root::VisibleRootRegistry;

fn record(key: &str, kind: &str, content: &str) -> serde_json::Value {
    json!({
        "idempotencyKey": key,
        "kind": kind,
        "content": content,
        "confidenceBasisPoints": 9000,
        "verification": "unverified",
        "importance": "normal",
        "rootPromotion": "candidate",
    })
}

fn batch_call(call_id: &str, records: Vec<serde_json::Value>) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: call_id.to_string(),
        tool_name: ToolName::plain("blackboard_record_batch"),
        model: "test-model".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(20_000),
        source: ToolCallSource::Direct,
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: json!({ "records": records }).to_string(),
        },
    }
}

fn summary(output: &dyn codex_extension_api::ToolOutput) -> serde_json::Value {
    let output =
        serde_json::from_str::<serde_json::Value>(&output.log_output()).expect("JSON output");
    json!({
        "recorded": output["recorded"],
        "alreadyPresent": output["alreadyPresent"],
        "failed": output["failed"],
        "errors": output["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|result| result["error"]
                .as_str()
                .map(|error| error.split(':').next().unwrap_or_default().to_string()))
            .collect::<Vec<_>>(),
    })
}

/// A repeated decision is a no-op that saves nothing new, a session-status summary is
/// refused, and genuinely new outcomes are still recorded.
#[tokio::test]
async fn exact_duplicates_and_status_summaries_add_nothing_while_new_outcomes_are_saved() {
    let state_home = TempDir::new().expect("state home");
    let tool = BlackboardBatchRecordTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs())),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        UserMessageRegistry::default(),
        VisibleRootRegistry::default(),
    );
    let decision = "Fix the vendored Click ordering, because both consumers read the source.";

    let first = tool
        .handle_call(batch_call(
            "first",
            vec![record("decision-1", "decision", decision)],
        ))
        .await
        .expect("first batch");
    let second = tool
        .handle_call(batch_call(
            "second",
            vec![
                record("decision-again", "decision", decision),
                record(
                    "status",
                    "fact",
                    "Yesterday's read-only investigation narrowed the bug to Click.",
                ),
                record(
                    "ruled-out",
                    "rejectedApproach",
                    "SHIPIT_CONFIG contamination: unset in both reproductions.",
                ),
            ],
        ))
        .await
        .expect("second batch");

    assert_eq!(
        [summary(first.as_ref()), summary(second.as_ref())],
        [
            json!({"recorded": 1, "alreadyPresent": 0, "failed": 0, "errors": [null]}),
            json!({
                "recorded": 1,
                "alreadyPresent": 1,
                "failed": 1,
                "errors": [null, "routineSummary", null],
            }),
        ]
    );
}
