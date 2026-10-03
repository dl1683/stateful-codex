use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulKnowledgeGroupCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeGroupItem;
use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryCounts;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use pretty_assertions::assert_eq;

use super::ExitCoverage;
use super::MemoryView;
use super::ServerLifetime;
use super::exit_lines;
use super::footer_text;
use super::status_cell;
use crate::history_cell::HistoryCell;

fn render(cell: &dyn HistoryCell, width: u16) -> String {
    cell.display_lines(width)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn counts() -> StatefulMemoryCounts {
    StatefulMemoryCounts {
        rules: 2,
        decisions: 1,
        open_checks: 1,
        background: 1,
        commits: 5,
        ..StatefulMemoryCounts::default()
    }
}

fn session() -> StatefulMemoryChangeTotals {
    StatefulMemoryChangeTotals {
        saved: 2,
        corrected: 1,
        forgotten: 1,
        commits_remembered: 3,
        ..StatefulMemoryChangeTotals::default()
    }
}

fn run(status: StatefulRunStatus) -> StatefulRun {
    StatefulRun {
        id: "run-1".to_string(),
        project_id: "project-1".to_string(),
        thread_ids: vec!["thread-1".to_string()],
        goal: "Finish the essay".to_string(),
        mode: StatefulWorkflowMode::Collaborative,
        budget: StatefulRunBudget {
            max_continuations: 0,
            max_elapsed_seconds: 0,
        },
        continuations_used: 0,
        status,
        strategy: None,
        strategy_revision: 0,
        result: None,
        revision: 1,
        created_at: 0,
        updated_at: 0,
    }
}

#[test]
fn footer_names_the_main_parts_and_sums_the_rest() {
    assert_eq!(
        (
            footer_text(&counts()),
            footer_text(&StatefulMemoryCounts::default()),
            footer_text(&StatefulMemoryCounts {
                rules: 1,
                ..StatefulMemoryCounts::default()
            }),
        ),
        (
            "Memory: 2 rules · 1 decision · 1 open check · 6 more".to_string(),
            "Memory: nothing saved yet".to_string(),
            "Memory: 1 rule".to_string(),
        )
    );
}

#[test]
fn status_and_exit_receipts() {
    let status = render(
        &status_cell(&counts(), Some(&session()), /*partial*/ false),
        /*width*/ 120,
    );
    let partial = render(
        &status_cell(&counts(), Some(&session()), /*partial*/ true),
        /*width*/ 120,
    );
    let quiet = render(
        &status_cell(
            &StatefulMemoryCounts::default(),
            Some(&StatefulMemoryChangeTotals::default()),
            /*partial*/ false,
        ),
        /*width*/ 120,
    );
    let exit = exit_lines(
        Some(&session()),
        ExitCoverage::Complete,
        Some(&run(StatefulRunStatus::Running)),
        ServerLifetime::Embedded,
    )
    .join("\n");
    let exit_persistent = exit_lines(
        Some(&session()),
        ExitCoverage::Partial,
        Some(&run(StatefulRunStatus::Paused)),
        ServerLifetime::Persistent,
    )
    .join("\n");
    let exit_quiet = exit_lines(
        Some(&StatefulMemoryChangeTotals::default()),
        ExitCoverage::Complete,
        Some(&run(StatefulRunStatus::Completed)),
        ServerLifetime::Embedded,
    )
    .join("\n");
    let exit_unknown = exit_lines(
        /*session*/ None,
        ExitCoverage::Partial,
        /*run*/ None,
        ServerLifetime::Embedded,
    )
    .join("\n");
    let unavailable = render(&MemoryView::Unavailable.status_cell(), /*width*/ 120);
    insta::assert_snapshot!(
        "memory_status_and_exit",
        [
            status,
            partial,
            quiet,
            unavailable,
            exit,
            exit_persistent,
            exit_quiet,
            exit_unknown,
        ]
        .join("\n---\n")
    );
}

/// Commits found in the workspace history say where they came from, never who made them;
/// commits already remembered say nothing.
#[test]
fn commit_receipts_name_the_workspace_history() {
    let item = |text: &str, outcome| StatefulKnowledgeGroupItem {
        entry_id: "entry".to_string(),
        revision: 1,
        category: StatefulKnowledgeCategory::Commit,
        outcome,
        text: text.to_string(),
    };
    let group = |saved, already, items| StatefulKnowledgeGroupCapturedNotification {
        project_id: "project-1".to_string(),
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        group_id: "group-1".to_string(),
        category: StatefulKnowledgeCategory::Commit,
        declared_count: None,
        recognized: saved + already,
        saved,
        already_present: already,
        pending: 0,
        omitted: 0,
        failed: 0,
        items,
        omitted_items: Vec::new(),
        scope_title: None,
    };
    let one = group(
        1,
        0,
        vec![item(
            "abc12345 Revise after editor feedback \u{2014} \u{a7} 3",
            StatefulCaptureOutcome::Stored,
        )],
    );
    let many = group(
        3,
        1,
        vec![
            item("abc12345 Draft section 3", StatefulCaptureOutcome::Stored),
            item("def67890 Draft conclusion", StatefulCaptureOutcome::Stored),
            item(
                "0011aabb Essay outline",
                StatefulCaptureOutcome::AlreadyStored,
            ),
        ],
    );
    let repeat = group(
        0,
        2,
        vec![item(
            "abc12345 Draft section 3",
            StatefulCaptureOutcome::AlreadyStored,
        )],
    );
    let render_group = |notification| {
        crate::stateful_memory::group_receipt_cell(notification)
            .map(|cell| render(&cell, /*width*/ 120))
            .unwrap_or_default()
    };
    insta::assert_snapshot!(
        "memory_commit_receipts",
        [render_group(&one), render_group(&many)].join("\n---\n")
    );
    assert_eq!(render_group(&repeat), String::new());
}
