use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootPromotion;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::ExecCommandSource;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::PatchApplyStatus;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::is_validation_command;
use super::journal_item;
use super::publish_window;
use crate::services::ProjectIntelligenceServices;

pub(crate) fn command_item(id: &str, command: &str, exit_code: i32, output: &str) -> TurnItem {
    let cwd = TempDir::new().expect("cwd");
    TurnItem::CommandExecution(CommandExecutionItem {
        model_context: None,
        sandbox_type: None,
        id: id.to_string(),
        plugin_id: None,
        script_path: None,
        process_id: None,
        command: vec!["bash".to_string(), "-lc".to_string(), command.to_string()],
        cwd: PathUri::from_abs_path(&cwd.path().abs()),
        parsed_cmd: Vec::new(),
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: if exit_code == 0 {
            CommandExecutionStatus::Completed
        } else {
            CommandExecutionStatus::Failed
        },
        stdout: Some(output.to_string()),
        stderr: Some(String::new()),
        aggregated_output: Some(output.to_string()),
        exit_code: Some(exit_code),
        duration: Some(Duration::from_millis(5)),
        formatted_output: None,
    })
}

pub(crate) fn edit_item(id: &str, path: PathBuf, status: PatchApplyStatus) -> TurnItem {
    TurnItem::FileChange(FileChangeItem {
        id: id.to_string(),
        changes: HashMap::from([(
            path,
            FileChange::Update {
                unified_diff: "@@ -1 +1 @@\n-a\n+b\n".to_string(),
                move_path: None,
            },
        )]),
        status: Some(status),
        auto_approved: None,
        stdout: None,
        stderr: None,
    })
}

pub(crate) fn message_item(id: &str, phase: MessagePhase, text: &str) -> TurnItem {
    TurnItem::AgentMessage(AgentMessageItem {
        id: id.to_string(),
        content: vec![AgentMessageContent::Text {
            text: text.to_string(),
        }],
        phase: Some(phase),
        memory_citation: None,
        delivery: None,
        questions: None,
    })
}

#[test]
fn validation_recognition_is_by_runner_name_only() {
    assert!(is_validation_command("bash -lc cargo test -p codex-core"));
    assert!(is_validation_command(
        "python -m pytest tests/test_scale.py"
    ));
    assert!(!is_validation_command("bash -lc cargo fmt"));
    assert!(!is_validation_command("git status"));
}

#[tokio::test]
async fn completed_items_are_journaled_redacted_and_published_once_per_window() {
    let home = TempDir::new().expect("home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let store = services.runtime().await.expect("runtime");
    let items = [
        edit_item(
            "patch-1",
            PathBuf::from("/work/src/scale.py"),
            PatchApplyStatus::Completed,
        ),
        command_item(
            "exec-1",
            "pytest -q",
            1,
            "FAILED test_eggs - Bearer abcdefghijklmnopqrstuvwxyz0123\n1 failed",
        ),
        message_item(
            "msg-1",
            MessagePhase::Commentary,
            "Next I will round eggs at output.",
        ),
    ];
    for item in &items {
        journal_item(store, "project-1", "thread-1", "turn-1", item).await;
    }
    // A replayed completion is the same observation.
    journal_item(store, "project-1", "thread-1", "turn-1", &items[1]).await;
    let events = store
        .window_events_newest_first("thread-1", 0, 10, 10)
        .await
        .expect("events");
    assert_eq!(events.len(), 3);
    let command = &events[1].event.payload;
    assert_eq!(command["exitCode"], json!(1));
    assert_eq!(command["status"], json!("failed"));
    let tail = command["outputTail"].as_str().expect("tail");
    assert!(tail.contains("1 failed"), "{tail}");
    assert!(!tail.contains("abcdefghijklmnopqrstuvwxyz0123"), "{tail}");
    assert_eq!(
        events[2].event.payload["paths"][0]["path"],
        json!(PathBuf::from("/work/src/scale.py").display().to_string())
    );

    publish_window(&services, "project-1", "thread-1").await;
    // Nothing new since the last publication: no second note.
    publish_window(&services, "project-1", "thread-1").await;
    let blackboard = services.blackboard().await.expect("blackboard");
    let notes = blackboard
        .query(codex_project_intelligence::BlackboardQuery {
            project_id: "project-1".to_string(),
            text: Some("Host-observed work receipts".to_string()),
            within_node: None,
            root_promotion: None,
            entry_scope: codex_project_intelligence::BlackboardEntryScope::Active,
            max_results: 10,
        })
        .await
        .expect("query")
        .data;
    assert_eq!(notes.len(), 1);
    let note = &notes[0].entry;
    assert_eq!(note.value.kind, BlackboardKind::Note);
    assert_eq!(note.value.root_promotion, RootPromotion::NotPromoted);
    assert_eq!(
        note.value.provenance.kind,
        BlackboardProvenanceKind::Maintenance
    );
    assert!(note.value.content.contains("scale.py (applied)"));
    assert!(note.value.content.contains("`bash -lc pytest -q` exit 1"));
    assert_eq!(
        store
            .pending_window_publications("project-1", 10)
            .await
            .expect("pending"),
        Vec::new()
    );
}

#[tokio::test]
async fn work_older_than_the_newest_page_still_publishes_and_message_only_windows_do_not() {
    let home = TempDir::new().expect("home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let store = services.runtime().await.expect("runtime");
    journal_item(
        store,
        "project-1",
        "thread-1",
        "turn-1",
        &command_item("exec-1", "cargo test", 1, "1 failed"),
    )
    .await;
    for index in 0..120 {
        journal_item(
            store,
            "project-1",
            "thread-1",
            "turn-1",
            &message_item(
                &format!("msg-{index}"),
                MessagePhase::Commentary,
                "Still reading the code.",
            ),
        )
        .await;
    }
    publish_window(&services, "project-1", "thread-1").await;
    // A second thread that only exchanged messages closes without a note.
    journal_item(
        store,
        "project-1",
        "thread-2",
        "turn-1",
        &message_item("msg-only", MessagePhase::Commentary, "Hello."),
    )
    .await;
    publish_window(&services, "project-1", "thread-2").await;

    let notes = services
        .blackboard()
        .await
        .expect("blackboard")
        .query(codex_project_intelligence::BlackboardQuery {
            project_id: "project-1".to_string(),
            text: Some("Host-observed work receipts".to_string()),
            within_node: None,
            root_promotion: None,
            entry_scope: codex_project_intelligence::BlackboardEntryScope::Active,
            max_results: 10,
        })
        .await
        .expect("query")
        .data;
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].entry.value.content.contains("thread-1"),
        "{}",
        notes[0].entry.value.content
    );
    assert_eq!(
        store
            .threads_with_unpublished_events("project-1", 10)
            .await
            .expect("threads"),
        Vec::<String>::new()
    );
}
