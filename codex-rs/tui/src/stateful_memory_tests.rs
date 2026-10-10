use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReplaced;
use codex_app_server_protocol::StatefulMemorySection;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use pretty_assertions::assert_eq;

use super::Footer;
use super::memory_lines;
use crate::history_cell::HistoryCell;

#[test]
fn memory_shortened_text_and_scope_states_are_explicit() {
    use codex_app_server_protocol::StatefulMemoryScopeState;
    let items = [
        StatefulMemoryScopeState::Open,
        StatefulMemoryScopeState::NotBoundHere,
        StatefulMemoryScopeState::Ended,
        StatefulMemoryScopeState::Unsupported,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, state)| {
        let mut item = item(
            &format!("rule-{index}"),
            StatefulMemorySection::UserRule,
            BlackboardKind::Instruction,
            "Preserve the full qualification of this rule.",
        );
        item.content_truncated = true;
        item.scope_state = Some(state);
        // Applied under an earlier, longer Apply bound: kept, not applied.
        item.exceeds_apply_bound = index == 0;
        (index + 1, item)
    })
    .collect::<Vec<_>>();
    insta::assert_snapshot!(
        "memory_shortened_scopes",
        render(memory_lines(&items, Footer::Complete, /*run*/ None))
    );
}

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
        authority: None,
        scope_state: None,
        attributed_to: None,
        exceeds_apply_bound: false,
    }
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
        item(
            "background",
            StatefulMemorySection::Background,
            BlackboardKind::Fact,
            "I'm a backend developer, mostly Go for the last six years.",
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
        render(memory_lines(
            &items
                .into_iter()
                .enumerate()
                .map(|(index, item)| (index + 1, item))
                .collect::<Vec<_>>(),
            Footer::More,
            Some(&run)
        ))
    );
    assert_eq!(
        render(memory_lines(&[], Footer::Complete, /*run*/ None)),
        "Project memory\n  Nothing saved yet."
    );
}

#[test]
fn memory_help_shows_explicit_targets() {
    let help = crate::history_cell::PlainHistoryCell::new(
        crate::stateful_memory_commands::HELP
            .iter()
            .map(|line| (*line).into())
            .collect(),
    );
    insta::assert_snapshot!("memory_help", render(help.display_lines(/*width*/ 180)));
}
