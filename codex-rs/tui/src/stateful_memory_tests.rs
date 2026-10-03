use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReplaced;
use codex_app_server_protocol::StatefulMemorySection;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use pretty_assertions::assert_eq;

use super::MemoryCommand;
use super::memory_lines;
use super::parse;
use super::receipt_cell;
use crate::history_cell::HistoryCell;

fn render(lines: Vec<ratatui::text::Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn item(
    entry_id: &str,
    section: StatefulMemorySection,
    kind: BlackboardKind,
    content: &str,
) -> StatefulMemoryItem {
    StatefulMemoryItem {
        entry_id: entry_id.to_string(),
        revision: 1,
        section,
        kind,
        content: content.to_string(),
        content_truncated: false,
        source: BlackboardProvenanceKind::User,
        updated_at: 1_790_000_000,
        replaces: Vec::new(),
    }
}

#[test]
fn memory_arguments_parse_into_commands() {
    assert_eq!(
        [
            "",
            "forget 2",
            "correct 1 Run only the affected tests.",
            "forget 0",
            "forget two",
            "correct 3",
            "drop 1",
        ]
        .map(parse),
        [
            Ok(MemoryCommand::List),
            Ok(MemoryCommand::Forget(2)),
            Ok(MemoryCommand::Correct(
                1,
                "Run only the affected tests.".to_string()
            )),
            Err(super::USAGE.to_string()),
            Err(super::USAGE.to_string()),
            Err(super::USAGE.to_string()),
            Err(super::USAGE.to_string()),
        ]
    );
}

#[test]
fn memory_listing_numbers_entries_by_section_and_names_the_open_run() {
    let mut decision = item(
        "decision",
        StatefulMemorySection::Decision,
        BlackboardKind::Decision,
        "Months use the symbol mth.",
    );
    decision.replaces = vec![StatefulMemoryReplaced {
        entry_id: "old".to_string(),
        content: "Months use the symbol mo.".to_string(),
        replaced_at: 1_789_000_000,
    }];
    let items = vec![
        item(
            "rule",
            StatefulMemorySection::UserRule,
            BlackboardKind::Instruction,
            "From now on, never run the whole test suite.",
        ),
        item(
            "pending",
            StatefulMemorySection::PendingRule,
            BlackboardKind::Instruction,
            "Don't modify any files today.",
        ),
        decision,
    ];
    let run = StatefulRun {
        id: "run-1".to_string(),
        project_id: "project-1".to_string(),
        thread_ids: vec!["thread-1".to_string()],
        goal: "Keep the formatter tidy".to_string(),
        mode: StatefulWorkflowMode::Collaborative,
        budget: StatefulRunBudget {
            max_continuations: 1,
            max_elapsed_seconds: 60,
        },
        continuations_used: 0,
        status: StatefulRunStatus::Running,
        strategy: None,
        strategy_revision: 0,
        result: None,
        revision: 1,
        created_at: 1,
        updated_at: 1,
    };
    insta::assert_snapshot!(
        "memory_listing",
        render(memory_lines(&items, /*more*/ false, Some(&run)))
    );
    assert_eq!(
        render(memory_lines(&[], /*more*/ false, /*run*/ None)),
        "Project memory\n  Nothing saved yet."
    );
}

#[test]
fn receipts_name_what_was_saved_and_repeats_say_nothing() {
    let notification = |category, outcome| StatefulKnowledgeCapturedNotification {
        project_id: "project-1".to_string(),
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        entry_id: "entry-1".to_string(),
        revision: 1,
        category,
        outcome,
        text: "From now on, never run the whole test suite.".to_string(),
    };
    let saved = receipt_cell(&notification(
        StatefulKnowledgeCategory::Rule,
        StatefulCaptureOutcome::Stored,
    ))
    .map(|cell| render(cell.display_lines(/*width*/ 100)));
    let pending = receipt_cell(&notification(
        StatefulKnowledgeCategory::PendingRule,
        StatefulCaptureOutcome::Stored,
    ))
    .map(|cell| render(cell.display_lines(/*width*/ 100)));
    let repeated = receipt_cell(&notification(
        StatefulKnowledgeCategory::Rule,
        StatefulCaptureOutcome::AlreadyStored,
    ))
    .is_none();
    insta::assert_snapshot!(
        "memory_receipts",
        format!(
            "{}\n{}",
            saved.unwrap_or_default(),
            pending.unwrap_or_default()
        )
    );
    assert!(repeated);
}
