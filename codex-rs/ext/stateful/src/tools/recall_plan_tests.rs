use std::sync::Arc;

use codex_extension_api::ConversationHistory;
use codex_extension_api::ExtensionData;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::RecallKind;
use crate::conversation_capture::CaptureTurn;
use crate::conversation_capture::capture_completed_answer;
use crate::conversation_capture::observe_agent_message;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";

/// debug2's five ruled-out items, as the session-1 answer listed them.
const DEBUG2_ANSWER: &str = "Root cause found: the vendored Click records the config parameter source late.

What was ruled out:

- Environment variable `SHIPIT_CONFIG`: absent during reproduction.
- Incorrect home-path expansion: the expected temporary home path was used.
- TOML parsing, built-in defaults, and deploy planning: the failure occurs before those stages; `load_config(None)` returns the built-in defaults.
- `--env` and `--dry-run`: both variants fail identically.
- Test/runtime code changes: the worktree remains clean.
";

#[test]
fn questions_name_the_kind_they_ask_for() {
    assert_eq!(
        [
            "What have we ruled out so far about the config error?",
            "Which hypotheses did we reject?",
            "What is still open before we can close the bug?",
            "Why did we pick SQLite?",
            "List the rules I gave you.",
            "What changed in the formatter?",
        ]
        .map(RecallKind::of_question),
        [
            Some(RecallKind::RuledOut),
            Some(RecallKind::RuledOut),
            Some(RecallKind::OpenCheck),
            Some(RecallKind::Decision),
            Some(RecallKind::Rule),
            None,
        ]
    );
}

fn agent_entry(
    node_id: &codex_project_intelligence::HierarchyNodeId,
    kind: BlackboardKind,
    content: &str,
) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: node_id.clone(),
        kind,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(8_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::Critical,
        root_promotion: RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "call-1".to_string(),
        },
    }
}

fn call(arguments: Value) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-9".to_string(),
        call_id: "recall-call".to_string(),
        tool_name: ToolName::plain("memory_read"),
        model: "test-model".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(20_000),
        source: ToolCallSource::Direct,
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: arguments.to_string(),
        },
    }
}

/// debug2 S5: after unrelated work fills memory with long, high-importance entries, one
/// bounded read of "what have we ruled out about the config error" returns all five
/// captured items first, in the answer's order, then the model's own legacy record on the
/// topic, and only after them another investigation's ruled-out items; the list is complete.
#[tokio::test]
async fn one_read_returns_every_ruled_out_item_on_the_topic() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services.project_node_id(PROJECT_ID).await.expect("node");
    let store = services.blackboard().await.expect("store");
    let id = |value: &str| BlackboardEntryId::parse(value).expect("id");
    store
        .create_entry(
            id("legacy-ruled-out"),
            agent_entry(
                &node_id,
                BlackboardKind::RejectedApproach,
                "Laptop setup differences ruled out: the config error reproduces in a clean home.",
            ),
        )
        .await
        .expect("legacy");
    for index in 0..12 {
        store
            .create_entry(
                id(&format!("unrelated-{index}")),
                agent_entry(
                    &node_id,
                    BlackboardKind::Decision,
                    &format!(
                        "Config error handling decision {index}: {}",
                        "ruled out nothing here; long unrelated detail about the config "
                            .repeat(8)
                            .trim_end()
                    ),
                ),
            )
            .await
            .expect("unrelated");
    }
    let turn = ExtensionData::new("turn");
    turn.insert(CaptureTurn {
        turn_id: "turn-1".to_string(),
    });
    observe_agent_message(
        &turn,
        &AgentMessageItem {
            id: "item-1".to_string(),
            content: vec![AgentMessageContent::Text {
                text: DEBUG2_ANSWER.to_string(),
            }],
            phase: None,
            memory_citation: None,
            delivery: None,
            questions: None,
        },
    );
    capture_completed_answer(&services, None, PROJECT_ID, "thread-1", &turn).await;
    let other = ExtensionData::new("turn");
    other.insert(CaptureTurn {
        turn_id: "turn-2".to_string(),
    });
    observe_agent_message(
        &other,
        &AgentMessageItem {
            id: "item-2".to_string(),
            content: vec![AgentMessageContent::Text {
                text: "Ruled out:\n- Token expiry: the auth token is fresh.\n- Clock skew: NTP is in sync.\n".to_string(),
            }],
            phase: None,
            memory_citation: None,
            delivery: None,
            questions: None,
        },
    );
    capture_completed_answer(&services, None, PROJECT_ID, "thread-2", &other).await;

    let tool = super::super::memory_read::MemoryReadTool::new(
        PROJECT_ID.to_string(),
        "thread-1".to_string(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
    );
    let output = tool
        .handle(call(json!({
            "question": "What have we ruled out so far about the config error?"
        })))
        .await
        .expect("recall");
    let result = serde_json::from_str::<Value>(&output.log_output()).expect("JSON");
    let requested = &result["requested"];
    assert_eq!(
        (
            requested["items"]
                .as_array()
                .expect("items")
                .iter()
                .map(|item| item["content"].as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>(),
            requested["coverage"]["complete"].clone(),
            requested["coverage"]["matching"].clone(),
            requested["coverage"]["onTopic"].clone(),
            requested["coverage"]["nextCursor"].clone(),
        ),
        (
            vec![
                "Environment variable `SHIPIT_CONFIG`: absent during reproduction.".to_string(),
                "Incorrect home-path expansion: the expected temporary home path was used."
                    .to_string(),
                "TOML parsing, built-in defaults, and deploy planning: the failure occurs before those stages; `load_config(None)` returns the built-in defaults.".to_string(),
                "`--env` and `--dry-run`: both variants fail identically.".to_string(),
                "Test/runtime code changes: the worktree remains clean.".to_string(),
                "Laptop setup differences ruled out: the config error reproduces in a clean home."
                    .to_string(),
                "Token expiry: the auth token is fresh.".to_string(),
                "Clock skew: NTP is in sync.".to_string(),
            ],
            json!(true),
            json!(8),
            json!(6),
            Value::Null,
        )
    );
    assert_eq!(
        requested["items"][0]["saidBy"],
        json!("the assistant's answer (reported, not verified)")
    );
}

/// A long list is paged without cutting an item, the cursor continues it, and a change to
/// the list in between restarts it from the first item, saying so.
#[tokio::test]
async fn a_long_list_pages_whole_items_with_a_cursor() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let items = (0..30)
        .map(|index| {
            format!(
                "- Hypothesis {index}: {}\n",
                "excluded by a repeatable experiment; ".repeat(6)
            )
        })
        .collect::<String>();
    let turn = ExtensionData::new("turn");
    turn.insert(CaptureTurn {
        turn_id: "turn-1".to_string(),
    });
    observe_agent_message(
        &turn,
        &AgentMessageItem {
            id: "item-1".to_string(),
            content: vec![AgentMessageContent::Text {
                text: format!("Ruled out:\n{items}"),
            }],
            phase: None,
            memory_citation: None,
            delivery: None,
            questions: None,
        },
    );
    capture_completed_answer(&services, None, PROJECT_ID, "thread-1", &turn).await;
    let tool = super::super::memory_read::MemoryReadTool::new(
        PROJECT_ID.to_string(),
        "thread-1".to_string(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
    );
    let mut seen = Vec::new();
    let mut cursor = Value::Null;
    let mut pages = 0;
    loop {
        let mut arguments = json!({"kind": "ruledOut"});
        if !cursor.is_null() {
            arguments["cursor"] = cursor.clone();
        }
        let output = tool.handle(call(arguments)).await.expect("page");
        let result = serde_json::from_str::<Value>(&output.log_output()).expect("JSON");
        let page = result["requested"]["items"]
            .as_array()
            .expect("items")
            .clone();
        assert!(
            !page.is_empty(),
            "every page carries at least one whole item"
        );
        seen.extend(
            page.iter()
                .map(|item| item["content"].as_str().unwrap_or_default().to_string()),
        );
        pages += 1;
        cursor = result["requested"]["coverage"]["nextCursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    let expected = (0..30)
        .map(|index| {
            format!(
                "Hypothesis {index}: {}",
                "excluded by a repeatable experiment; ".repeat(6).trim_end()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!((seen, pages > 1), (expected, true));

    let output = tool
        .handle(call(json!({"kind": "ruledOut", "cursor": "stale:3"})))
        .await
        .expect("restart");
    let result = serde_json::from_str::<Value>(&output.log_output()).expect("JSON");
    assert_eq!(
        (
            result["requested"]["coverage"]["startAt"].clone(),
            result["requested"]["coverage"]["cursorNote"].is_string()
        ),
        (json!(0), true)
    );
}
