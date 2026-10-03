use std::sync::Arc;

use codex_extension_api::ConversationHistory;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseItem;
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
    batch_call_with_history(call_id, records, ConversationHistory::default())
}

fn batch_call_with_history(
    call_id: &str,
    records: Vec<serde_json::Value>,
    conversation_history: ConversationHistory,
) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: call_id.to_string(),
        tool_name: ToolName::plain("blackboard_record_batch"),
        model: "test-model".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(20_000),
        source: ToolCallSource::Direct,
        conversation_history,
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

/// A recipe whose command the host saw exit 0 stores the conditions it ran under and is
/// labelled current; one the host never saw run stays an unverified fact; a recipe that
/// carries a credential is refused.
#[tokio::test]
async fn recipes_are_grounded_only_in_commands_the_host_saw_succeed() {
    let state_home = TempDir::new().expect("state home");
    let repo = TempDir::new().expect("repo");
    std::fs::write(
        repo.path().join("pyproject.toml"),
        "[project]
name='shipit'
",
    )
    .expect("manifest");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let tool = BlackboardBatchRecordTool::new(
        "project-1".to_string(),
        "thread-1".to_string(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        UserMessageRegistry::default(),
        VisibleRootRegistry::default(),
    );
    services.observed_commands().started(
        "thread-1",
        crate::recipe_capture::ObservedCommand {
            turn_id: "turn-1".to_string(),
            call_id: "exec-1".to_string(),
            script: "cd . && python -m pytest tests/test_cli.py -q".to_string(),
            cwd: repo.path().to_path_buf(),
        },
    );
    services
        .observed_commands()
        .finished("exec-1", /*succeeded*/ true);
    let history = ConversationHistory::new(vec![ResponseItem::FunctionCallOutput {
        id: None,
        call_id: Some("exec-1".to_string()),
        name: None,
        namespace: None,
        output: FunctionCallOutputPayload::from_text(
            "Wall time: 1.0 seconds
Process exited with code 0
Output:
3 passed
"
            .to_string(),
        ),
        internal_chat_message_metadata_passthrough: None,
    }]);

    let output = tool
        .handle_call(batch_call_with_history(
            "recipes",
            vec![
                record(
                    "observed",
                    "fact",
                    "Recipe: `python -m pytest tests/test_cli.py -q` runs the CLI tests.",
                ),
                record("unseen", "fact", "Recipe: `make test` runs everything."),
                record(
                    "secret",
                    "fact",
                    "Recipe: `deploy --token=abc123` publishes the site.",
                ),
            ],
            history,
        ))
        .await
        .expect("recipe batch");
    let output =
        serde_json::from_str::<serde_json::Value>(&output.log_output()).expect("JSON output");
    let labels = output["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|result| {
            result["recipe"]
                .as_str()
                .or_else(|| result["error"].as_str())
                .and_then(|label| label.split([' ', ':']).next())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let observed_id = codex_project_intelligence::BlackboardEntryId::parse(
        output["results"][0]["entryId"].as_str().expect("entry id"),
    )
    .expect("valid entry id");
    let context = services
        .blackboard()
        .await
        .expect("blackboard")
        .knowledge_context("project-1", &observed_id)
        .await
        .expect("context loads")
        .expect("recipe context stored");
    let observation = serde_json::from_str::<crate::recipe_capture::RecipeObservation>(
        context.payload.as_deref().expect("payload"),
    )
    .expect("observation");

    assert_eq!(
        (
            labels,
            context.category,
            context.authority,
            observation.exit_status,
            observation
                .manifests
                .iter()
                .map(|manifest| manifest.path.as_str())
                .collect::<Vec<_>>(),
            crate::recipe_applicability::check_observation(&observation),
        ),
        (
            vec![
                Some("current".to_string()),
                Some("notObserved".to_string()),
                Some("credentials".to_string()),
            ],
            codex_project_intelligence::KnowledgeCategory::Recipe,
            codex_project_intelligence::KnowledgeAuthority::HostObserved,
            crate::recipe_capture::ExitStatus::Zero,
            vec!["pyproject.toml"],
            crate::recipe_applicability::RecipeCheck::Current,
        )
    );
}
