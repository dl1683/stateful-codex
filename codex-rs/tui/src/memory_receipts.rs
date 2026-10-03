//! How project memory's state reads in the TUI: the passive footer text, the `/status` lines,
//! the exit receipt and the dated return recap. Pure rendering of what the app server counted
//! from its journal of committed changes; no counts are kept or guessed here.

use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryCounts;
use codex_app_server_protocol::StatefulMemoryRecapResponse;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::history_cell::PlainHistoryCell;
use crate::stateful_memory::preview;

/// Parts named in the footer before the rest is summed as "more".
const FOOTER_PARTS: usize = 3;

fn plural(count: u32, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

/// Every nonzero part of current memory, most useful first.
fn count_parts(counts: &StatefulMemoryCounts) -> Vec<(u32, String)> {
    [
        (counts.rules, "rule", "rules"),
        (counts.decisions, "decision", "decisions"),
        (counts.open_checks, "open check", "open checks"),
        (counts.background, "note about you", "notes about you"),
        (
            counts.pending_rules,
            "task-limited rule",
            "task-limited rules",
        ),
        (counts.commits, "remembered commit", "remembered commits"),
        (
            counts.unverified_rules,
            "rule not in your words",
            "rules not in your words",
        ),
        (counts.other, "other entry", "other entries"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, one, many)| (count, plural(count, one, many)))
    .collect()
}

/// The compact footer text: `Memory: 2 rules · 1 decision · 1 open check · 4 more`.
pub(crate) fn footer_text(counts: &StatefulMemoryCounts) -> String {
    let parts = count_parts(counts);
    if parts.is_empty() {
        return "Memory: nothing saved yet".to_string();
    }
    let mut shown = parts
        .iter()
        .take(FOOTER_PARTS)
        .map(|(_, text)| text.clone())
        .collect::<Vec<_>>();
    let rest = parts
        .iter()
        .skip(FOOTER_PARTS)
        .map(|(count, _)| *count)
        .sum::<u32>();
    if rest > 0 {
        shown.push(format!("{rest} more"));
    }
    format!("Memory: {}", shown.join(" · "))
}

/// What a stretch of the journal changed, nonzero parts only.
fn change_parts(totals: &StatefulMemoryChangeTotals) -> Vec<String> {
    [
        (totals.saved, "saved"),
        (totals.promoted, "now applied"),
        (totals.corrected, "corrected"),
        (totals.forgotten, "forgotten"),
        (totals.invalidated, "no longer current"),
        (totals.scopes_ended, "investigations ended"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .chain((totals.commits_remembered > 0).then(|| {
        plural(
            totals.commits_remembered,
            "commit remembered from workspace history",
            "commits remembered from workspace history",
        )
    }))
    .chain((totals.capture_incomplete > 0).then(|| {
        plural(
            totals.capture_incomplete,
            "capture could not finish",
            "captures could not finish",
        )
    }))
    .collect()
}

/// The `/status` lines: the full breakdown of current memory and this session's changes.
pub(crate) fn status_cell(
    counts: &StatefulMemoryCounts,
    session: Option<&StatefulMemoryChangeTotals>,
) -> PlainHistoryCell {
    let parts = count_parts(counts)
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>();
    let held = if parts.is_empty() {
        "nothing saved yet".to_string()
    } else {
        parts.join(" · ")
    };
    let mut lines: Vec<Line<'static>> = vec![vec![" Project memory: ".bold(), held.into()].into()];
    if let Some(session) = session {
        let changes = change_parts(session);
        let changed = if changes.is_empty() {
            "nothing saved or changed yet".to_string()
        } else {
            changes.join(" · ")
        };
        lines.push(vec![" This session: ".dim(), changed.dim()].into());
    }
    lines.push(
        " /memory to review · counted from saved changes"
            .dim()
            .into(),
    );
    PlainHistoryCell::new(lines)
}

/// The memory part of the exit receipt: what this session changed, and what an open run
/// does after the user quits.
pub(crate) fn exit_lines(
    session: Option<&StatefulMemoryChangeTotals>,
    run: Option<&StatefulRun>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(session) = session {
        let changes = change_parts(session);
        lines.push(if changes.is_empty() {
            "Project memory: nothing was saved or changed this session.".to_string()
        } else {
            format!("Project memory this session: {}.", changes.join(", "))
        });
    }
    if let Some(run) = run
        && matches!(
            run.status,
            StatefulRunStatus::Pending | StatefulRunStatus::Running | StatefulRunStatus::Paused
        )
    {
        let mode = match run.mode {
            StatefulWorkflowMode::Autonomous => "Autonomous",
            StatefulWorkflowMode::Collaborative => "Collaborative",
            StatefulWorkflowMode::Socratic => "Socratic",
        };
        lines.push(format!(
            "Your {mode} run stays open for next time; nothing works on it while you are away."
        ));
    }
    lines
}

/// The dated return card, or `None` when the project has no finished work to return to.
pub(crate) fn recap_cell(
    recap: &StatefulMemoryRecapResponse,
    format_time: &dyn Fn(i64) -> String,
) -> Option<PlainHistoryCell> {
    let work = recap.last_work.as_ref()?;
    let mut lines: Vec<Line<'static>> = vec![
        vec![
            "Where things stand".bold(),
            format!(" · as of {}", format_time(recap.as_of)).dim(),
        ]
        .into(),
    ];
    let request = work
        .request
        .as_deref()
        .map(|request| format!(": {}", preview(request)))
        .unwrap_or_default();
    lines.push(format!("  Last finished {}{request}", format_time(work.finished_at)).into());
    if !recap.rules.is_empty() {
        lines.push(
            format!(
                "  Your rules ({}):",
                recap.rules.len() + recap.more_rules as usize
            )
            .into(),
        );
        for (index, rule) in recap.rules.iter().enumerate() {
            lines.push(format!("    {}. {}", index + 1, preview(rule)).dim().into());
        }
        if recap.more_rules > 0 {
            lines.push(
                format!("    and {} more · /memory", recap.more_rules)
                    .dim()
                    .into(),
            );
        }
    }
    for decision in &recap.decisions {
        let reason = decision
            .reason
            .as_deref()
            .map(|reason| format!(" because {}", preview(reason)))
            .unwrap_or_else(|| " (no reason recorded)".to_string());
        lines.push(format!("  Decision: {}{reason}", preview(&decision.text)).into());
    }
    for check in &recap.open_checks {
        lines.push(format!("  Open check: {}", preview(check)).into());
    }
    if !recap.commits.is_empty() {
        lines.push(
            format!(
                "  Recently remembered from workspace history: {}",
                recap.commits.join(" · ")
            )
            .dim()
            .into(),
        );
    }
    let more = recap.more_decisions + recap.more_open_checks;
    if more > 0 {
        let what = if more == 1 {
            "decision or check"
        } else {
            "decisions or checks"
        };
        lines.push(format!("  {more} more {what} · /memory").dim().into());
    }
    if recap.capture_incomplete > 0 {
        lines.push(
            format!(
                "  {} since then; something you said may be missing · /memory add",
                plural(
                    recap.capture_incomplete,
                    "capture could not finish",
                    "captures could not finish"
                )
            )
            .dim()
            .into(),
        );
    }
    lines.push(
        "  No next step was recorded; say where you want to pick up."
            .dim()
            .into(),
    );
    Some(PlainHistoryCell::new(lines))
}

#[cfg(test)]
#[path = "memory_receipts_tests.rs"]
mod tests;
