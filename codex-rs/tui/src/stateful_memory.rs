//! How project memory looks in the TUI: the `/memory` listing and the quiet receipts shown
//! when a turn saves something. The commands themselves live in `stateful_memory_commands`.

use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemorySection;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::history_cell::PlainHistoryCell;

/// The `/memory` listing: the run line, then numbered sections in review order.
/// What the listing's last line says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Footer {
    /// Every entry has been shown.
    Complete,
    /// More entries follow; `/memory more` shows them.
    More,
}

/// The `/memory` listing of numbered rows, grouped by section.
pub(crate) fn memory_lines(
    items: &[(usize, StatefulMemoryItem)],
    footer: Footer,
    run: Option<&StatefulRun>,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("Project memory".bold())];
    if let Some(run) = run {
        lines.push(vec!["  ".into(), run_line(run).dim()].into());
    }
    if items.is_empty() {
        lines.push("  Nothing saved yet.".dim().into());
        return lines;
    }
    let sections = [
        (StatefulMemorySection::UserRule, "Your retained rules"),
        (
            StatefulMemorySection::PendingRule,
            "Task-limited rules (kept, not applied)",
        ),
        (
            StatefulMemorySection::UnverifiedRule,
            "Rules not in your words (not applied)",
        ),
        (StatefulMemorySection::Background, "About you (your words)"),
        (StatefulMemorySection::Decision, "Decisions"),
        (StatefulMemorySection::Knowledge, "Other knowledge"),
    ];
    for (section, title) in sections {
        let numbered = items
            .iter()
            .filter(|(_, item)| item.section == section)
            .collect::<Vec<_>>();
        if numbered.is_empty() {
            continue;
        }
        lines.push(Line::from(""));
        lines.push(Line::from(title.bold()));
        for (number, item) in numbered {
            lines.push(vec![format!("  {number}. ").dim(), item.content.clone().into()].into());
            lines.push(
                format!("     {}@{}", item.entry_id, item.revision)
                    .dim()
                    .into(),
            );
            // The user's own memory is never readable through model tools, so no line points
            // there.
            if item.exceeds_apply_bound {
                let shown = if item.content_truncated {
                    " (only its first 2,000 bytes are shown)"
                } else {
                    ""
                };
                lines.push(
                    format!(
                        "     not applied: longer than 240 bytes; re-add with /memory add{shown}"
                    )
                    .dim()
                    .into(),
                );
            } else if item.content_truncated {
                lines.push(
                    "     Shortened: only its first 2,000 bytes are shown"
                        .dim()
                        .into(),
                );
            }
            if let Some(replaced) = item.replaces.first() {
                lines.push(vec!["     replaces: ".dim(), preview(&replaced.content).dim()].into());
            }
            if let Some(state) = item.scope_state {
                lines.push(
                    format!("     investigation: {}", scope_state(Some(state)))
                        .dim()
                        .into(),
                );
            }
            if let Some(speaker) = &item.attributed_to {
                lines.push(
                    format!("     {speaker}'s words you passed on, not your rule")
                        .dim()
                        .into(),
                );
            }
        }
    }
    if footer == Footer::More {
        lines.push("  More entries follow: /memory more".dim().into());
    }
    lines.push(Line::from(""));
    lines.push(
        "  List numbers are display conveniences. Use the shown ID@REV for changes."
            .dim()
            .into(),
    );
    lines.push(
        "  /memory add · /memory forget <ID@REV> · /memory correct <ID@REV> <new text> · /memory help · no model turn is used"
            .dim()
            .into(),
    );
    lines
}

fn run_line(run: &StatefulRun) -> String {
    let mode = match run.mode {
        StatefulWorkflowMode::Autonomous => "Autonomous",
        StatefulWorkflowMode::Collaborative => "Collaborative",
        StatefulWorkflowMode::Socratic => "Socratic",
    };
    let status = match run.status {
        StatefulRunStatus::Pending => "starting",
        StatefulRunStatus::Running => "open",
        StatefulRunStatus::Paused => "paused",
        StatefulRunStatus::Completed => "completed",
        StatefulRunStatus::Cancelled => "cancelled",
        StatefulRunStatus::Blocked => "blocked",
        StatefulRunStatus::Failed => "failed",
        StatefulRunStatus::Answered => "answered · not verified",
    };
    match run.status {
        StatefulRunStatus::Pending | StatefulRunStatus::Running | StatefulRunStatus::Paused => {
            format!("{mode} run {status} · it stays open between answers and after you quit")
        }
        StatefulRunStatus::Completed
        | StatefulRunStatus::Cancelled
        | StatefulRunStatus::Blocked
        | StatefulRunStatus::Failed
        | StatefulRunStatus::Answered => format!("{mode} run {status}"),
    }
}

/// Where a section is shown in the listing.
pub(crate) fn section_rank(section: StatefulMemorySection) -> u8 {
    match section {
        StatefulMemorySection::UserRule => 0,
        StatefulMemorySection::PendingRule => 1,
        StatefulMemorySection::UnverifiedRule => 2,
        StatefulMemorySection::Background => 3,
        StatefulMemorySection::Decision => 4,
        StatefulMemorySection::Knowledge => 5,
    }
}

pub(crate) fn section_noun(section: StatefulMemorySection) -> &'static str {
    match section {
        StatefulMemorySection::UserRule => "retained rule",
        StatefulMemorySection::PendingRule => "task-limited rule (not applied)",
        StatefulMemorySection::UnverifiedRule => "rule (not applied)",
        StatefulMemorySection::Decision => "decision",
        StatefulMemorySection::Background => "note about you",
        StatefulMemorySection::Knowledge => "entry",
    }
}

pub(crate) fn preview(text: &str) -> String {
    const MAX_CHARS: usize = 120;
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= MAX_CHARS {
        return format!("\"{single}\"");
    }
    let cut = single.chars().take(MAX_CHARS).collect::<String>();
    // End at a word boundary when one is near, so the preview never stops mid-word.
    let cut = match cut.rfind(' ') {
        Some(space) if space > cut.len() / 2 => cut[..space].trim_end(),
        Some(_) | None => cut.as_str(),
    };
    format!("\"{cut}…\"")
}

/// One quiet line for a newly saved model record; repeats say nothing.
pub(crate) fn receipt_cell(
    notification: &StatefulKnowledgeCapturedNotification,
) -> Option<PlainHistoryCell> {
    if notification.outcome == StatefulCaptureOutcome::AlreadyStored {
        return None;
    }
    let what = match notification.category {
        StatefulKnowledgeCategory::Decision => "Saved a decision",
        StatefulKnowledgeCategory::Recipe => "Saved a project recipe",
        StatefulKnowledgeCategory::Finding => "Saved a finding",
    };
    Some(PlainHistoryCell::new(vec![
        vec![
            "• ".dim(),
            format!("{what}: {}", preview(&notification.text)).dim(),
            " · /memory to review".dark_gray(),
        ]
        .into(),
    ]))
}

#[cfg(test)]
#[path = "stateful_memory_tests.rs"]
mod tests;

fn scope_state(state: Option<codex_app_server_protocol::StatefulMemoryScopeState>) -> &'static str {
    use codex_app_server_protocol::StatefulMemoryScopeState;
    match state {
        Some(StatefulMemoryScopeState::Unsupported) => "unsupported; held back",
        Some(StatefulMemoryScopeState::Open) => "open, bound here",
        Some(StatefulMemoryScopeState::NotBoundHere) => "not bound here",
        Some(StatefulMemoryScopeState::Ended) => "ended",
        Some(StatefulMemoryScopeState::Unknown) | None => "unknown",
    }
}
