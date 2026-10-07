//! How project memory looks in the TUI: the `/memory` listing and the quiet receipts shown
//! when a turn saves something. The commands themselves live in `stateful_memory_commands`.

use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulKnowledgeGroupCapturedNotification;
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
            lines.push(
                format!("     {}@{}", item.entry_id, item.revision)
                    .dim()
                    .into(),
            );
            if let Some(replaced) = item.replaces.first() {
                lines.push(vec!["     replaces: ".dim(), preview(&replaced.content).dim()].into());
            }
            if let Some(scope) = &item.scope_title {
                lines.push(
                    vec![
                        "     only in the investigation: ".dim(),
                        preview(scope).dim(),
                    ]
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

/// The counted receipt for everything one message's capture committed: how many rules were
/// saved, each by its opening words, and anything already saved, kept but not applied, too
/// long to keep, or failed. A capture that changed nothing and lost nothing says nothing.
pub(crate) fn group_receipt_cell(
    notification: &StatefulKnowledgeGroupCapturedNotification,
) -> Option<PlainHistoryCell> {
    let changed = notification.saved + notification.pending;
    let lost = notification.omitted + notification.failed;
    if changed == 0 && lost == 0 {
        return None;
    }
    let plural = |count: u32, one: &str, many: &str| {
        if count == 1 {
            format!("{count} {one}")
        } else {
            format!("{count} {many}")
        }
    };
    let headline = match (notification.saved, notification.pending) {
        (0, 0) => "Nothing new was saved".to_string(),
        (saved, 0) => format!("Saved {}", plural(saved, "rule", "rules")),
        (0, pending) => format!(
            "Saved {} for this task only (not applied later)",
            plural(pending, "rule", "rules")
        ),
        (saved, pending) => format!(
            "Saved {} and {} for this task only",
            plural(saved, "rule", "rules"),
            plural(pending, "rule", "rules")
        ),
    };
    let mut lines: Vec<Line<'static>> = vec![
        vec![
            "• ".dim(),
            headline.dim(),
            " · /memory to review".dark_gray(),
        ]
        .into(),
    ];
    let mut number = 0;
    for item in &notification.items {
        if item.outcome == StatefulCaptureOutcome::AlreadyStored {
            continue;
        }
        number += 1;
        let note = match item.category {
            StatefulKnowledgeCategory::PendingRule => " (this task only)",
            StatefulKnowledgeCategory::Rule
            | StatefulKnowledgeCategory::Decision
            | StatefulKnowledgeCategory::Recipe
            | StatefulKnowledgeCategory::Finding
            | StatefulKnowledgeCategory::Background => "",
        };
        lines.push(
            format!("    {number}. {}{note}", preview(rule_body(&item.text)))
                .dim()
                .into(),
        );
    }
    if let Some(scope) = &notification.scope_title {
        lines.push(
            format!("    For the investigation: {}", preview(scope))
                .dim()
                .into(),
        );
    }
    if notification.already_present > 0 {
        lines.push(
            format!(
                "    {} already saved",
                plural(notification.already_present, "rule was", "rules were")
            )
            .dim()
            .into(),
        );
    }
    for omitted in &notification.omitted_items {
        lines.push(
            format!(
                "    Not saved, too long to keep whole: {}",
                preview(omitted)
            )
            .dim()
            .into(),
        );
    }
    if notification.failed > 0 {
        lines.push(
            format!(
                "    {} could not be saved",
                plural(notification.failed, "rule", "rules")
            )
            .dim()
            .into(),
        );
    }
    let recognized = notification.recognized;
    if let Some(declared) = notification.declared_count
        && declared != recognized
    {
        lines.push(
            format!(
                "    You mentioned {declared}; {recognized} were recognized. /memory add rule <text> adds a missing one."
            )
            .dim()
            .into(),
        );
    }
    Some(PlainHistoryCell::new(lines))
}

#[cfg(test)]
#[path = "stateful_memory_tests.rs"]
mod tests;

/// Investigation controls show explicit identities alongside read-only display numbers.
pub(crate) fn scope_lines(
    response: &codex_app_server_protocol::StatefulMemoryScopeResponse,
    done: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines: Vec<ratatui::text::Line<'static>> = Vec::new();
    if let Some(done) = done {
        lines.push(done.to_string().into());
    }
    if response.scopes.is_empty() {
        lines.push(
            "No investigations yet. Rules you give \"for this whole investigation\" start one."
                .into(),
        );
    }
    for (index, scope) in response.scopes.iter().enumerate() {
        let state = match (scope.open, scope.this_thread) {
            (true, true) => "open · this thread",
            (true, false) => "open",
            (false, _) => "ended",
        };
        lines.push(
            format!(
                "  {}. {} ({state}) · {}",
                index + 1,
                preview(&scope.title),
                scope.scope_id
            )
            .into(),
        );
        if let Some(condition) = &scope.end_condition {
            lines.push(format!("     ends: {condition}").into());
        }
    }
    lines.push("List numbers are display conveniences. /memory join <scope-ID> · /memory end <scope-ID> · /memory leave".dim().into());
    lines
}
