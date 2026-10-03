//! `/memory`: review, forget and correct project memory with no model turn, and the quiet
//! receipts shown when a turn saves something to it.
//!
//! Item numbers refer to the last listing shown for the same thread; any change made here
//! retires that listing, so a number can never name an entry the user did not see.

use std::sync::Arc;
use std::sync::Mutex;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulCaptureOutcome;
use codex_app_server_protocol::StatefulKnowledgeCapturedNotification;
use codex_app_server_protocol::StatefulKnowledgeCategory;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_app_server_protocol::StatefulMemorySection;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_protocol::ThreadId;
use ratatui::style::Stylize;
use ratatui::text::Line;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::new_error_event;
use crate::history_cell::new_info_event;

/// Entries one `/memory` listing shows (two pages).
const MAX_LISTED: usize = 100;
const PAGE_SIZE: u32 = 50;
const USAGE: &str = "Usage: /memory, /memory forget <number>, /memory correct <number> <new text>";

/// What the user asked `/memory` to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemoryCommand {
    List,
    Forget(usize),
    Correct(usize, String),
}

/// Parses the arguments of `/memory`.
pub(crate) fn parse(args: &str) -> Result<MemoryCommand, String> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(MemoryCommand::List);
    }
    let (verb, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let rest = rest.trim();
    let (number, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let number = number
        .parse::<usize>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| USAGE.to_string());
    match verb.to_ascii_lowercase().as_str() {
        "forget" if text.trim().is_empty() => Ok(MemoryCommand::Forget(number?)),
        "correct" if !text.trim().is_empty() => {
            Ok(MemoryCommand::Correct(number?, text.trim().to_string()))
        }
        _ => Err(USAGE.to_string()),
    }
}

/// The entries of the last listing, by the number shown, for one thread.
#[derive(Clone, Debug, Default)]
pub(crate) struct MemoryListing {
    shown: Arc<Mutex<Option<ShownListing>>>,
}

#[derive(Clone, Debug)]
struct ShownListing {
    thread_id: String,
    items: Vec<StatefulMemoryItem>,
}

impl MemoryListing {
    fn replace(&self, listing: Option<ShownListing>) {
        *self
            .shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = listing;
    }

    fn item(&self, thread_id: &str, number: usize) -> Result<StatefulMemoryItem, String> {
        let shown = self
            .shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(listing) = shown.as_ref().filter(|shown| shown.thread_id == thread_id) else {
            return Err("Run /memory first; numbers refer to the list it shows.".to_string());
        };
        listing
            .items
            .get(number - 1)
            .cloned()
            .ok_or_else(|| format!("There is no item {number} in the last /memory list."))
    }
}

/// Runs one `/memory` command in the background and shows its result in the transcript.
pub(crate) fn run(
    request_handle: AppServerRequestHandle,
    listing: MemoryListing,
    thread_id: Option<ThreadId>,
    args: String,
    app_event_tx: AppEventSender,
) {
    tokio::spawn(async move {
        let insert = |cell: PlainHistoryCell| {
            app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
        };
        let Some(thread_id) = thread_id.map(|thread_id| thread_id.to_string()) else {
            insert(new_error_event(
                "Project memory needs a session; start or resume one first.".to_string(),
            ));
            return;
        };
        let command = match parse(&args) {
            Ok(command) => command,
            Err(usage) => {
                insert(new_error_event(usage));
                return;
            }
        };
        let result = match command {
            MemoryCommand::List => list(&request_handle, &listing, &thread_id).await,
            MemoryCommand::Forget(number) => {
                forget(&request_handle, &listing, &thread_id, number).await
            }
            MemoryCommand::Correct(number, text) => {
                correct(&request_handle, &listing, &thread_id, number, text).await
            }
        };
        insert(result.unwrap_or_else(new_error_event));
    });
}

async fn list(
    request_handle: &AppServerRequestHandle,
    listing: &MemoryListing,
    thread_id: &str,
) -> Result<PlainHistoryCell, String> {
    let mut items = Vec::new();
    let mut cursor = None;
    let mut more = false;
    loop {
        let page: StatefulMemoryReadResponse = request_handle
            .request_typed(ClientRequest::StatefulMemoryRead {
                request_id: request_id("read"),
                params: StatefulMemoryReadParams {
                    thread_id: thread_id.to_string(),
                    cursor,
                    limit: Some(PAGE_SIZE),
                },
            })
            .await
            .map_err(|error| format!("Could not read project memory: {error}"))?;
        items.extend(page.data);
        match page.next_cursor {
            Some(next) if items.len() < MAX_LISTED => cursor = Some(next),
            Some(_) => {
                more = true;
                break;
            }
            None => break,
        }
    }
    items.truncate(MAX_LISTED);
    let run = read_run(request_handle, thread_id).await;
    listing.replace(Some(ShownListing {
        thread_id: thread_id.to_string(),
        items: items.clone(),
    }));
    Ok(PlainHistoryCell::new(memory_lines(
        &items,
        more,
        run.as_ref(),
    )))
}

async fn forget(
    request_handle: &AppServerRequestHandle,
    listing: &MemoryListing,
    thread_id: &str,
    number: usize,
) -> Result<PlainHistoryCell, String> {
    let item = listing.item(thread_id, number)?;
    let _: StatefulMemoryForgetResponse = request_handle
        .request_typed(ClientRequest::StatefulMemoryForget {
            request_id: request_id("forget"),
            params: StatefulMemoryForgetParams {
                thread_id: thread_id.to_string(),
                entry_id: item.entry_id.clone(),
                expected_revision: item.revision,
            },
        })
        .await
        .map_err(|error| format!("Nothing was forgotten: {error}"))?;
    listing.replace(None);
    Ok(new_info_event(
        format!("Forgot: {}", preview(&item.content)),
        Some("It stays in history but no longer applies. Run /memory to see the list.".to_string()),
    ))
}

async fn correct(
    request_handle: &AppServerRequestHandle,
    listing: &MemoryListing,
    thread_id: &str,
    number: usize,
    text: String,
) -> Result<PlainHistoryCell, String> {
    let item = listing.item(thread_id, number)?;
    let response: StatefulMemoryCorrectResponse = request_handle
        .request_typed(ClientRequest::StatefulMemoryCorrect {
            request_id: request_id("correct"),
            params: StatefulMemoryCorrectParams {
                thread_id: thread_id.to_string(),
                entry_id: item.entry_id.clone(),
                expected_revision: item.revision,
                content: text,
            },
        })
        .await
        .map_err(|error| format!("Nothing was corrected: {error}"))?;
    listing.replace(None);
    Ok(new_info_event(
        format!(
            "Corrected {}: {}",
            section_noun(response.item.section),
            preview(&response.item.content)
        ),
        Some(format!("Replaces: {}", preview(&item.content))),
    ))
}

async fn read_run(request_handle: &AppServerRequestHandle, thread_id: &str) -> Option<StatefulRun> {
    request_handle
        .request_typed::<StatefulRunReadResponse>(ClientRequest::StatefulRunRead {
            request_id: request_id("run-read"),
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(thread_id.to_string()),
            },
        })
        .await
        .ok()
        .and_then(|response| response.run)
}

fn request_id(action: &str) -> RequestId {
    RequestId::String(format!(
        "stateful-tui-memory-{action}-{}",
        uuid::Uuid::new_v4()
    ))
}

/// The `/memory` listing: the run line, then numbered sections in review order.
pub(crate) fn memory_lines(
    items: &[StatefulMemoryItem],
    more: bool,
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
        (StatefulMemorySection::Decision, "Decisions"),
        (StatefulMemorySection::Knowledge, "Other knowledge"),
    ];
    for (section, title) in sections {
        let numbered = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.section == section)
            .collect::<Vec<_>>();
        if numbered.is_empty() {
            continue;
        }
        lines.push(Line::from(""));
        lines.push(Line::from(title.bold()));
        for (index, item) in numbered {
            lines.push(
                vec![
                    format!("  {}. ", index + 1).dim(),
                    item.content.clone().into(),
                ]
                .into(),
            );
            if let Some(replaced) = item.replaces.first() {
                lines.push(vec!["     replaces: ".dim(), preview(&replaced.content).dim()].into());
            }
        }
    }
    if more {
        lines.push(
            format!("  Showing the first {MAX_LISTED} entries.")
                .dim()
                .into(),
        );
    }
    lines.push(Line::from(""));
    lines.push(
        "  /memory forget <number> · /memory correct <number> <new text> · no model turn is used"
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

fn section_noun(section: StatefulMemorySection) -> &'static str {
    match section {
        StatefulMemorySection::UserRule => "rule (applied)",
        StatefulMemorySection::PendingRule => "task-limited rule (not applied)",
        StatefulMemorySection::UnverifiedRule => "rule (not applied)",
        StatefulMemorySection::Decision => "decision",
        StatefulMemorySection::Knowledge => "entry",
    }
}

fn preview(text: &str) -> String {
    const MAX_CHARS: usize = 120;
    let single = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.chars().count() <= MAX_CHARS {
        return format!("\"{single}\"");
    }
    let cut = single.chars().take(MAX_CHARS).collect::<String>();
    format!("\"{cut}…\"")
}

/// Framing that introduces a message's rules ("Two standing rules for all our work here:"):
/// it names rules or preferences and is not itself an instruction.
fn rule_body(text: &str) -> &str {
    const MAX_FRAMING_CHARS: usize = 100;
    const MIN_BODY_CHARS: usize = 20;
    const FRAMING_NOUNS: &[&str] = &["rule", "rules", "preference", "preferences"];
    const INSTRUCTION_WORDS: &[&str] = &["never", "always", "don't", "do", "must", "should"];
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
        && !words
            .iter()
            .any(|word| INSTRUCTION_WORDS.contains(&word.as_str()))
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
