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
        (
            StatefulMemorySection::UserRule,
            "Your rules (applied to all work)",
        ),
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
            if let Some(replaced) = item.replaces.first() {
                lines.push(vec!["     replaces: ".dim(), preview(&replaced.content).dim()].into());
            }
        }
    }
    if footer == Footer::More {
        lines.push("  More entries follow: /memory more".dim().into());
    }
    lines.push(Line::from(""));
    lines.push(
        "  /memory add · /memory forget <number> · /memory correct <number> <new text> · /memory help · no model turn is used"
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
    };
    match run.status {
        StatefulRunStatus::Pending | StatefulRunStatus::Running | StatefulRunStatus::Paused => {
            format!("{mode} run {status} · it stays open between answers and after you quit")
        }
        StatefulRunStatus::Completed
        | StatefulRunStatus::Cancelled
        | StatefulRunStatus::Blocked
        | StatefulRunStatus::Failed => format!("{mode} run {status}"),
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
        StatefulMemorySection::UserRule => "rule (applied)",
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

/// Framing that introduces a message's rules ("Two standing rules for all our work here:"):
/// it names rules or preferences and is not itself an instruction.
fn rule_body(text: &str) -> &str {
    const MAX_FRAMING_CHARS: usize = 100;
    const MIN_BODY_CHARS: usize = 20;
    const FRAMING_NOUNS: &[&str] = &["rule", "rules", "preference", "preferences"];
    const INSTRUCTION_WORDS: &[&str] = &["never", "always", "don't", "do", "must", "should"];
    // Framing that limits where the rules apply is part of what was saved.
    const SCOPE_WORDS: &[&str] = &[
        "investigation",
        "bug",
        "issue",
        "incident",
        "until",
        "today",
        "task",
        "week",
    ];
    let Some((framing, body)) = text.split_once(": ") else {
        return text;
    };
    let words = framing
        .split(|character: char| !(character.is_alphanumeric() || character == '\''))
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let recognised = framing.chars().count() <= MAX_FRAMING_CHARS
        && !framing.contains(['"', '\'', '`', '\u{201c}', '\u{2018}'])
        && words
            .iter()
            .any(|word| FRAMING_NOUNS.contains(&word.as_str()))
        && !words.iter().any(|word| {
            INSTRUCTION_WORDS.contains(&word.as_str()) || SCOPE_WORDS.contains(&word.as_str())
        })
        && body.trim().chars().count() >= MIN_BODY_CHARS;
    if recognised { body.trim() } else { text }
}

/// What a session saved to project memory, so each rule receipt of a turn carries its number
/// ("Saved your rule 2 of this message").
#[derive(Debug, Default)]
pub(crate) struct ReceiptTally {
    /// The turn the last rule receipt belonged to, and how many rules it saved so far.
    rules_in_turn: Option<(String, usize)>,
}

impl ReceiptTally {
    /// One quiet line for something a turn newly saved; repeats of what was already saved
    /// say nothing.
    pub(crate) fn receipt_cell(
        &mut self,
        notification: &StatefulKnowledgeCapturedNotification,
    ) -> Option<PlainHistoryCell> {
        if notification.outcome == StatefulCaptureOutcome::AlreadyStored {
            return None;
        }
        let rule_number = match notification.category {
            StatefulKnowledgeCategory::Rule | StatefulKnowledgeCategory::PendingRule => {
                let number = match &mut self.rules_in_turn {
                    Some((turn_id, count)) if *turn_id == notification.turn_id => {
                        *count += 1;
                        *count
                    }
                    _ => {
                        self.rules_in_turn = Some((notification.turn_id.clone(), 1));
                        1
                    }
                };
                Some(number)
            }
            StatefulKnowledgeCategory::Decision
            | StatefulKnowledgeCategory::Recipe
            | StatefulKnowledgeCategory::Finding
            | StatefulKnowledgeCategory::Background => None,
        };
        Some(receipt_cell(notification, rule_number))
    }
}

/// The receipt line; `rule_number` counts the rules saved from the same message.
fn receipt_cell(
    notification: &StatefulKnowledgeCapturedNotification,
    rule_number: Option<usize>,
) -> PlainHistoryCell {
    let what = match (notification.category, rule_number) {
        (StatefulKnowledgeCategory::Rule, Some(number)) if number > 1 => {
            format!("Saved your rule {number} from this message")
        }
        (StatefulKnowledgeCategory::Rule, _) => "Saved your rule".to_string(),
        (StatefulKnowledgeCategory::PendingRule, _) => {
            "Saved a task-limited rule (not applied)".to_string()
        }
        (StatefulKnowledgeCategory::Decision, _) => "Saved a decision".to_string(),
        (StatefulKnowledgeCategory::Recipe, _) => "Saved a project recipe".to_string(),
        (StatefulKnowledgeCategory::Finding, _) => "Saved a finding".to_string(),
        (StatefulKnowledgeCategory::Background, _) => {
            "Saved what you said about yourself".to_string()
        }
    };
    let text = match notification.category {
        StatefulKnowledgeCategory::Rule | StatefulKnowledgeCategory::PendingRule => {
            rule_body(&notification.text)
        }
        StatefulKnowledgeCategory::Decision
        | StatefulKnowledgeCategory::Recipe
        | StatefulKnowledgeCategory::Finding
        | StatefulKnowledgeCategory::Background => &notification.text,
    };
    PlainHistoryCell::new(vec![
        vec![
            "• ".dim(),
            format!("{what}: {}", preview(text)).dim(),
            " · /memory to review".dark_gray(),
        ]
        .into(),
    ])
}

#[cfg(test)]
#[path = "stateful_memory_tests.rs"]
mod tests;
