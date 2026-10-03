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

use super::publish_window;
use crate::services::ProjectIntelligenceServices;
use crate::window_capture::journal_item;

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
fn validation_recognition_is_by_what_runs() {
    use crate::runner::invoked_runner;
    assert_eq!(
        invoked_runner("cargo test -p codex-core"),
        Some("cargo test")
    );
    assert_eq!(
        invoked_runner("python -m pytest tests/test_scale.py"),
        Some("pytest")
    );
    assert_eq!(invoked_runner("cargo fmt"), None);
    assert_eq!(invoked_runner("git status"), None);
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
        .window_events_newest_first("thread-1", "project-1", 0, 10, 10)
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

    publish_window(&services, "project-1", "thread-1")
        .await
        .expect("published");
    // Nothing new since the last publication: no second note.
    publish_window(&services, "project-1", "thread-1")
        .await
        .expect("published");
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
    assert!(
        note.value
            .content
            .contains("Paths in patches that applied, newest first:")
    );
    assert!(note.value.content.contains("scale.py;"));
    assert!(
        note.value
            .content
            .contains("Last failing test or check command (exit 1): `pytest -q`.")
    );
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
    publish_window(&services, "project-1", "thread-1")
        .await
        .expect("published");
    // A second thread that only exchanged messages closes without a note.
    journal_item(
        store,
        "project-1",
        "thread-2",
        "turn-1",
        &message_item("msg-only", MessagePhase::Commentary, "Hello."),
    )
    .await;
    publish_window(&services, "project-1", "thread-2")
        .await
        .expect("published");

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
            .threads_with_unpublished_events("project-1", i64::MAX, 10)
            .await
            .expect("threads"),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn long_paths_never_crowd_out_the_test_and_failure_receipts() {
    let home = TempDir::new().expect("home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let store = services.runtime().await.expect("runtime");
    for index in 0..8 {
        journal_item(
            store,
            "project-1",
            "thread-1",
            "turn-1",
            &edit_item(
                &format!("patch-{index}"),
                PathBuf::from(format!("/work/{index}/{}", "d".repeat(480))),
                PatchApplyStatus::Failed,
            ),
        )
        .await;
    }
    journal_item(
        store,
        "project-1",
        "thread-1",
        "turn-1",
        &command_item("exec-1", "pytest -q", 1, "1 failed"),
    )
    .await;
    let events = store
        .window_events_newest_first("thread-1", "project-1", 0, 9, 64)
        .await
        .expect("events");
    let content = super::publication_content("thread-1", 0, 9, &events);
    assert!(content.len() <= 3_800);
    assert!(
        content.contains("Last failing test or check command (exit 1): `pytest -q`."),
        "{content}"
    );
    assert!(
        content.contains(
            "Paths in patches that failed (attempted; which files changed was not observed)"
        ),
        "{content}"
    );
    assert!(content.contains("more not listed."), "{content}");
}

#[test]
fn a_long_failing_command_keeps_its_exit_code_in_the_publication() {
    let long = format!("pytest {}", "k".repeat(950));
    let event = codex_stateful_runtime::WindowEvent {
        seq: 1,
        event: codex_stateful_runtime::NewWindowEvent {
            thread_id: "thread-1".to_string(),
            event_key: "exec-1:command".to_string(),
            project_id: "project-1".to_string(),
            turn_id: "turn-1".to_string(),
            kind: codex_stateful_runtime::WindowEventKind::Command,
            payload: json!({"command": long, "status": "failed", "exitCode": 17, "validation": true}),
        },
        created_at_ms: 0,
    };
    let content = super::publication_content("thread-1", 0, 1, &[event]);
    assert!(
        content.contains("Last failing test or check command (exit 17): `pytest"),
        "{content}"
    );
    assert!(
        content.contains("Last command without exit code 0 (exit 17):"),
        "{content}"
    );
}

#[tokio::test]
async fn recovery_closes_message_only_sentinels_and_retries_closed_windows() {
    let home = TempDir::new().expect("home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let store = services.runtime().await.expect("runtime");
    // A message-only suffix staged but interrupted before its acknowledgement.
    journal_item(
        store,
        "project-1",
        "thread-1",
        "turn-1",
        &message_item("m1", MessagePhase::Commentary, "Reading."),
    )
    .await;
    store
        .stage_window_publication(&codex_stateful_runtime::WindowPublication {
            thread_id: "thread-1".to_string(),
            from_seq: 0,
            through_seq: 1,
            project_id: "project-1".to_string(),
            entry_id: "stateful-window-receipts-sentinel".to_string(),
            content: "no receipts".to_string(),
            state: codex_stateful_runtime::WindowPublicationState::Pending,
        })
        .await
        .expect("stage sentinel");
    // A closed window whose staging never happened.
    journal_item(
        store,
        "project-1",
        "thread-2",
        "turn-1",
        &command_item("exec-1", "cargo test", 1, "1 failed"),
    )
    .await;
    store
        .record_window_closure("thread-2", "project-1", 1)
        .await
        .expect("closure");

    super::recover_pending(&services, "project-1").await;

    let notes = services
        .blackboard()
        .await
        .expect("blackboard")
        .query(codex_project_intelligence::BlackboardQuery {
            project_id: "project-1".to_string(),
            text: None,
            within_node: None,
            root_promotion: None,
            entry_scope: codex_project_intelligence::BlackboardEntryScope::Active,
            max_results: 10,
        })
        .await
        .expect("query")
        .data;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].entry.value.content.contains("thread-2"));
    assert_eq!(
        store
            .pending_window_publications("project-1", 10)
            .await
            .expect("pending"),
        Vec::new()
    );
}

/// Thread A closed a window through event 1 but never staged it, then kept working (event 2,
/// open). A new thread B of the project publishes A's closed suffix and nothing of its open
/// work, although A is too recent for idle recovery.
#[tokio::test]
async fn a_new_thread_publishes_another_threads_closed_window_but_not_its_open_work() {
    let home = TempDir::new().expect("home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let store = services.runtime().await.expect("runtime");
    journal_item(
        store,
        "project-1",
        "thread-a",
        "turn-1",
        &command_item("exec-1", "cargo test", 1, "1 failed"),
    )
    .await;
    store
        .record_window_closure("thread-a", "project-1", 1)
        .await
        .expect("closure");
    journal_item(
        store,
        "project-1",
        "thread-a",
        "turn-2",
        &command_item("exec-2", "cargo build", 0, "ok"),
    )
    .await;

    super::publish_open_windows(&services, "project-1").await;

    assert_eq!(
        store
            .window_publication_watermark("thread-a", "project-1")
            .await
            .expect("watermark"),
        1
    );
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
    assert!(notes[0].entry.value.content.contains("events 1-1"));
    assert!(!notes[0].entry.value.content.contains("cargo build"));
}
