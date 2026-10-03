//! `/memory` commands: list, page, add, forget and correct project memory with no model turn.
//!
//! Numbers are stable for a listing: correcting item 3 keeps it item 3 (now the corrected
//! text), forgetting it leaves 3 unused, adding or paging appends new numbers. A new `/memory`
//! starts a new listing. Every result names the thread and listing it belongs to, so a reply
//! that arrives after the user switched threads, or after a newer listing, changes nothing it
//! does not own.

use std::sync::Arc;
use std::sync::Mutex;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulMemoryAddKind;
use codex_app_server_protocol::StatefulMemoryAddOutcome;
use codex_app_server_protocol::StatefulMemoryAddParams;
use codex_app_server_protocol::StatefulMemoryAddResponse;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
use codex_app_server_protocol::StatefulMemoryScopeAction;
use codex_app_server_protocol::StatefulMemoryScopeParams;
use codex_app_server_protocol::StatefulMemoryScopeResponse;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_protocol::ThreadId;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::new_error_event;
use crate::history_cell::new_info_event;
use crate::stateful_memory::Footer;
use crate::stateful_memory::memory_lines;
use crate::stateful_memory::preview;
use crate::stateful_memory::section_noun;
use crate::stateful_memory::section_rank;

/// Entries one page shows.
const PAGE_SIZE: u32 = 50;
const USAGE: &str = "Usage: /memory, /memory next, /memory add <rule|about-me|decision|note> <text>, /memory forget <number>, /memory correct <number> <new text>, /memory investigations, /memory help";

/// What the user asked `/memory` to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemoryCommand {
    List,
    More,
    Help,
    /// The project's investigations, numbered.
    Investigations,
    /// This thread continues investigation N of the last investigations list.
    Join(usize),
    /// End investigation N of the last investigations list.
    End(usize),
    /// This thread no longer continues an investigation.
    Leave,
    Add(Addition),
    Forget(usize),
    Correct(usize, String),
}

/// An entry the user adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Addition {
    pub(crate) kind: StatefulMemoryAddKind,
    pub(crate) content: String,
    /// A rule's scope ("for this investigation, until we agree:").
    pub(crate) scope: Option<String>,
    /// A decision's reason ("... because ...").
    pub(crate) reason: Option<String>,
}

/// Parses the arguments of `/memory`.
pub(crate) fn parse(args: &str) -> Result<MemoryCommand, String> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(MemoryCommand::List);
    }
    let (verb, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let rest = rest.trim();
    match verb.to_ascii_lowercase().as_str() {
        "more" | "next" if rest.is_empty() => return Ok(MemoryCommand::More),
        "refresh" | "list" if rest.is_empty() => return Ok(MemoryCommand::List),
        "help" if rest.is_empty() => return Ok(MemoryCommand::Help),
        "investigations" if rest.is_empty() => return Ok(MemoryCommand::Investigations),
        "leave" if rest.is_empty() => return Ok(MemoryCommand::Leave),
        "add" => return parse_addition(rest).map(MemoryCommand::Add),
        _ => {}
    }
    let (number, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let number = number
        .parse::<usize>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| USAGE.to_string());
    match verb.to_ascii_lowercase().as_str() {
        "forget" if text.trim().is_empty() => Ok(MemoryCommand::Forget(number?)),
        "join" if text.trim().is_empty() => Ok(MemoryCommand::Join(number?)),
        "end" if text.trim().is_empty() => Ok(MemoryCommand::End(number?)),
        "correct" if !text.trim().is_empty() => {
            Ok(MemoryCommand::Correct(number?, text.trim().to_string()))
        }
        _ => Err(USAGE.to_string()),
    }
}

/// `rule [for <scope>:] <text>`, `about <text>`, `decision <choice> [because <reason>]`,
/// `note <text>`.
fn parse_addition(rest: &str) -> Result<Addition, String> {
    const ADD_USAGE: &str = "Usage: /memory add rule <text> (or: rule for <scope>: <text>), /memory add about-me <text>, /memory add decision <choice> because <reason>, /memory add note <text>";
    let (kind, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let text = text.trim();
    if text.is_empty() {
        return Err(ADD_USAGE.to_string());
    }
    let addition = |kind, content: &str, scope: Option<&str>, reason: Option<&str>| Addition {
        kind,
        content: content.trim().to_string(),
        scope: scope.map(|scope| scope.trim().to_string()),
        reason: reason.map(|reason| reason.trim().to_string()),
    };
    match kind.to_ascii_lowercase().as_str() {
        "rule" => {
            let scoped = text
                .strip_prefix("for ")
                .and_then(|scoped| scoped.split_once(": "));
            Ok(match scoped {
                Some((scope, rule)) if !rule.trim().is_empty() => addition(
                    StatefulMemoryAddKind::Rule,
                    rule,
                    Some(&format!("For {scope}")),
                    None,
                ),
                Some(_) | None => addition(StatefulMemoryAddKind::Rule, text, None, None),
            })
        }
        "about" | "about-me" | "background" | "me" => Ok(addition(
            StatefulMemoryAddKind::Background,
            text,
            None,
            None,
        )),
        "decision" => Ok(match text.split_once(" because ") {
            Some((choice, reason)) => {
                addition(StatefulMemoryAddKind::Decision, choice, None, Some(reason))
            }
            None => addition(StatefulMemoryAddKind::Decision, text, None, None),
        }),
        "note" => Ok(addition(StatefulMemoryAddKind::Note, text, None, None)),
        _ => Err(ADD_USAGE.to_string()),
    }
}

/// The help shown by `/memory help`.
pub(crate) const HELP: &[&str] = &[
    "/memory - list what this project remembers (no model turn)",
    "/memory next - show the next entries of a long list (/memory refresh lists again)",
    "/memory add rule <text> - add a rule in your words; rule for <scope>: <text> limits it",
    "/memory add about-me <text> - add something about you",
    "/memory add decision <choice> because <reason> - add a decision with its reason",
    "/memory add note <text> - add anything else worth keeping",
    "/memory correct <number> <text> - replace an entry with your words",
    "/memory forget <number> - stop using an entry (it stays in history)",
    "/memory investigations - list investigations; /memory join <n>, /memory end <n>, /memory leave",
    "Numbers stay the same until you list again. Outside the TUI: codex memory --help",
];

/// One numbered row of a listing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Slot {
    pub(crate) item: StatefulMemoryItem,
    pub(crate) forgotten: bool,
}

/// The listing shown for one thread: its rows by number, and where paging continues.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Listing {
    pub(crate) thread_id: String,
    pub(crate) generation: u64,
    pub(crate) slots: Vec<Slot>,
    pub(crate) cursor: Option<String>,
}

impl Listing {
    /// Appends a page: its rows sorted into section order among themselves, numbered after
    /// every row already shown. Returns the numbers given.
    pub(crate) fn append(&mut self, mut items: Vec<StatefulMemoryItem>) -> Vec<usize> {
        items.sort_by_key(|item| section_rank(item.section));
        let first = self.slots.len() + 1;
        let numbers = (first..first + items.len()).collect();
        self.slots.extend(items.into_iter().map(|item| Slot {
            item,
            forgotten: false,
        }));
        numbers
    }

    /// The current row `number`, if it is still in use.
    pub(crate) fn current(&self, number: usize) -> Result<&StatefulMemoryItem, String> {
        match number
            .checked_sub(1)
            .and_then(|index| self.slots.get(index))
        {
            None => Err(format!(
                "There is no item {number} in the last /memory list."
            )),
            Some(slot) if slot.forgotten => Err(format!("Item {number} was forgotten.")),
            Some(slot) => Ok(&slot.item),
        }
    }

    /// Records a change to the row holding `entry_id`, wherever it now is.
    fn update(&mut self, entry_id: &str, change: impl FnOnce(&mut Slot)) {
        if let Some(slot) = self
            .slots
            .iter_mut()
            .find(|slot| slot.item.entry_id == entry_id)
        {
            change(slot);
        }
    }
}

/// The listings by thread; a result applies only to the listing it was made for.
#[derive(Clone, Debug, Default)]
pub(crate) struct MemoryListing {
    shown: Arc<Mutex<Option<Listing>>>,
    next_generation: Arc<Mutex<u64>>,
    /// The last investigations list shown: its generation, thread, and scope IDs by number.
    investigations: Arc<Mutex<Option<(u64, String, Vec<String>)>>>,
}

impl MemoryListing {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Listing>> {
        self.shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn begin(&self) -> u64 {
        let mut next = self
            .next_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *next += 1;
        *next
    }

    /// The listing for `thread_id`, if one is shown.
    fn for_thread(&self, thread_id: &str) -> Result<Listing, String> {
        self.lock()
            .as_ref()
            .filter(|listing| listing.thread_id == thread_id)
            .cloned()
            .ok_or_else(|| "Run /memory first; numbers refer to the list it shows.".to_string())
    }

    /// Applies `change` to the listing if it is still generation `generation` of the thread.
    fn with_current<T>(
        &self,
        thread_id: &str,
        generation: u64,
        change: impl FnOnce(&mut Listing) -> T,
    ) -> Option<T> {
        self.lock()
            .as_mut()
            .filter(|listing| listing.thread_id == thread_id && listing.generation == generation)
            .map(change)
    }
}

/// Runs one `/memory` command in the background and shows its result in the thread it was
/// asked in.
pub(crate) fn run(
    request_handle: AppServerRequestHandle,
    listing: MemoryListing,
    thread_id: Option<ThreadId>,
    args: String,
    app_event_tx: AppEventSender,
) {
    tokio::spawn(async move {
        let Some(thread) = thread_id else {
            app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(new_error_event(
                "Project memory needs a session; start or resume one first.".to_string(),
            ))));
            return;
        };
        let insert = |cell: PlainHistoryCell| {
            app_event_tx.send(AppEvent::StatefulMemoryResult {
                thread_id: thread,
                cell: Box::new(cell),
            });
        };
        let thread_id = thread.to_string();
        let command = match parse(&args) {
            Ok(command) => command,
            Err(usage) => {
                insert(new_error_event(usage));
                return;
            }
        };
        let request = Requests {
            handle: &request_handle,
            thread_id: &thread_id,
        };
        let result = match command {
            MemoryCommand::List => list(&request, &listing).await,
            MemoryCommand::More => more(&request, &listing).await,
            MemoryCommand::Help => Ok(PlainHistoryCell::new(
                HELP.iter().map(|line| (*line).into()).collect(),
            )),
            MemoryCommand::Add(addition) => add(&request, &listing, addition).await,
            MemoryCommand::Investigations => {
                investigations(&request, &listing, StatefulMemoryScopeAction::List, None).await
            }
            MemoryCommand::Leave => {
                investigations(&request, &listing, StatefulMemoryScopeAction::Leave, None).await
            }
            MemoryCommand::Join(number) => {
                investigations(
                    &request,
                    &listing,
                    StatefulMemoryScopeAction::Join,
                    Some(number),
                )
                .await
            }
            MemoryCommand::End(number) => {
                investigations(
                    &request,
                    &listing,
                    StatefulMemoryScopeAction::End,
                    Some(number),
                )
                .await
            }
            MemoryCommand::Forget(number) => forget(&request, &listing, number).await,
            MemoryCommand::Correct(number, text) => correct(&request, &listing, number, text).await,
        };
        insert(result.unwrap_or_else(new_error_event));
    });
}

/// The app-server handle and the thread a command acts for.
struct Requests<'a> {
    handle: &'a AppServerRequestHandle,
    thread_id: &'a str,
}

impl Requests<'_> {
    async fn page(&self, cursor: Option<String>) -> Result<StatefulMemoryReadResponse, String> {
        self.handle
            .request_typed(ClientRequest::StatefulMemoryRead {
                request_id: request_id("read"),
                params: StatefulMemoryReadParams {
                    thread_id: self.thread_id.to_string(),
                    cursor,
                    limit: Some(PAGE_SIZE),
                    background_section: true,
                },
            })
            .await
            .map_err(|error| format!("Could not read project memory: {error}"))
    }

    async fn run(&self) -> Option<StatefulRun> {
        self.handle
            .request_typed::<StatefulRunReadResponse>(ClientRequest::StatefulRunRead {
                request_id: request_id("run-read"),
                params: StatefulRunReadParams {
                    run_id: None,
                    thread_id: Some(self.thread_id.to_string()),
                },
            })
            .await
            .ok()
            .and_then(|response| response.run)
    }
}

async fn list(request: &Requests<'_>, listing: &MemoryListing) -> Result<PlainHistoryCell, String> {
    let generation = listing.begin();
    let page = request.page(/*cursor*/ None).await?;
    let run = request.run().await;
    let mut shown = Listing {
        thread_id: request.thread_id.to_string(),
        generation,
        slots: Vec::new(),
        cursor: page.next_cursor,
    };
    shown.append(page.data);
    let lines = memory_lines(&numbered(&shown, 1), footer(&shown), run.as_ref());
    // A newer listing started meanwhile owns the numbers; this older one is not shown, so
    // no visible number can point at a different entry.
    {
        let mut current = listing.lock();
        let newer = current
            .as_ref()
            .is_some_and(|current| current.generation > generation);
        if newer {
            return Ok(new_info_event(
                "A newer /memory list replaced this one; use its numbers.".to_string(),
                /*hint*/ None,
            ));
        }
        *current = Some(shown);
    }
    Ok(PlainHistoryCell::new(lines))
}

async fn more(request: &Requests<'_>, listing: &MemoryListing) -> Result<PlainHistoryCell, String> {
    let shown = listing.for_thread(request.thread_id)?;
    let Some(cursor) = shown.cursor.clone() else {
        return Ok(new_info_event(
            "That was the whole list.".to_string(),
            /*hint*/ None,
        ));
    };
    let page = request.page(Some(cursor)).await.map_err(|error| {
        format!("{error}. Memory changed since the list was shown; run /memory to list it again.")
    })?;
    listing
        .with_current(request.thread_id, shown.generation, |current| {
            let first = current.slots.len() + 1;
            current.cursor = page.next_cursor;
            current.append(page.data);
            memory_lines(
                &numbered(current, first),
                footer(current),
                /*run*/ None,
            )
        })
        .map(PlainHistoryCell::new)
        .ok_or_else(|| "A newer /memory list replaced this one; use its numbers.".to_string())
}

async fn add(
    request: &Requests<'_>,
    listing: &MemoryListing,
    addition: Addition,
) -> Result<PlainHistoryCell, String> {
    let response: StatefulMemoryAddResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryAdd {
            request_id: request_id("add"),
            params: StatefulMemoryAddParams {
                thread_id: request.thread_id.to_string(),
                kind: addition.kind,
                content: addition.content,
                scope: addition.scope,
                reason: addition.reason,
                client_action_id: uuid::Uuid::new_v4().to_string(),
                background_section: true,
            },
        })
        .await
        .map_err(|error| format!("Nothing was added: {error}"))?;
    let item = response.item;
    // A shown listing gets the new row under the next number.
    let number = {
        let mut current = listing.lock();
        current
            .as_mut()
            .filter(|current| {
                current.thread_id == request.thread_id
                    && !current
                        .slots
                        .iter()
                        .any(|slot| slot.item.entry_id == item.entry_id && !slot.forgotten)
            })
            .and_then(|current| current.append(vec![item.clone()]).first().copied())
    };
    let what = match response.outcome {
        StatefulMemoryAddOutcome::Added => format!("Added {}", section_noun(item.section)),
        StatefulMemoryAddOutcome::AlreadyPresent | StatefulMemoryAddOutcome::AlreadyDone => {
            format!("Already saved as {}", section_noun(item.section))
        }
    };
    let hint = number.map(|number| format!("It is item {number} in the /memory list."));
    Ok(new_info_event(
        format!("{what}: {}", preview(&item.content)),
        hint,
    ))
}

async fn forget(
    request: &Requests<'_>,
    listing: &MemoryListing,
    number: usize,
) -> Result<PlainHistoryCell, String> {
    let shown = listing.for_thread(request.thread_id)?;
    let item = shown.current(number)?.clone();
    let _: StatefulMemoryForgetResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryForget {
            request_id: request_id("forget"),
            params: StatefulMemoryForgetParams {
                thread_id: request.thread_id.to_string(),
                entry_id: item.entry_id.clone(),
                expected_revision: item.revision,
            },
        })
        .await
        .map_err(|error| format!("Nothing was forgotten: {error}"))?;
    listing.with_current(request.thread_id, shown.generation, |current| {
        current.update(&item.entry_id, |slot| slot.forgotten = true);
    });
    Ok(new_info_event(
        format!("Forgot item {number}: {}", preview(&item.content)),
        Some("It stays in history but no longer applies. Other numbers are unchanged.".to_string()),
    ))
}

async fn correct(
    request: &Requests<'_>,
    listing: &MemoryListing,
    number: usize,
    text: String,
) -> Result<PlainHistoryCell, String> {
    let shown = listing.for_thread(request.thread_id)?;
    let item = shown.current(number)?.clone();
    let response: StatefulMemoryCorrectResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryCorrect {
            request_id: request_id("correct"),
            params: StatefulMemoryCorrectParams {
                thread_id: request.thread_id.to_string(),
                entry_id: item.entry_id.clone(),
                expected_revision: item.revision,
                content: text,
                background_section: true,
            },
        })
        .await
        .map_err(|error| format!("Nothing was corrected: {error}"))?;
    let corrected = response.item;
    listing.with_current(request.thread_id, shown.generation, |current| {
        let successor = corrected.clone();
        current.update(&item.entry_id, |slot| slot.item = successor);
    });
    Ok(new_info_event(
        format!(
            "Corrected item {number} ({}): {}",
            section_noun(corrected.section),
            preview(&corrected.content)
        ),
        Some(format!("Replaces: {}", preview(&item.content))),
    ))
}

/// Lists, joins, leaves or ends an investigation; numbers refer to the last investigations
/// list shown for this thread.
async fn investigations(
    request: &Requests<'_>,
    listing: &MemoryListing,
    action: StatefulMemoryScopeAction,
    number: Option<usize>,
) -> Result<PlainHistoryCell, String> {
    let scope_id = match number {
        Some(number) => {
            let shown = listing
                .investigations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let Some((_, _, scopes)) = shown.filter(|(_, thread, _)| thread == request.thread_id)
            else {
                return Err(
                    "Run /memory investigations first; numbers refer to its list.".to_string(),
                );
            };
            Some(
                scopes.get(number - 1).cloned().ok_or_else(|| {
                    format!("There is no investigation {number} in the last list.")
                })?,
            )
        }
        None => None,
    };
    let generation = listing.begin();
    let response: StatefulMemoryScopeResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryScope {
            request_id: request_id("scope"),
            params: StatefulMemoryScopeParams {
                thread_id: request.thread_id.to_string(),
                action,
                scope_id,
            },
        })
        .await
        .map_err(|error| format!("Nothing changed: {error}"))?;
    // Only the newest investigations list owns the numbers.
    {
        let mut shown = listing
            .investigations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if shown
            .as_ref()
            .is_none_or(|(shown_generation, _, _)| *shown_generation < generation)
        {
            *shown = Some((
                generation,
                request.thread_id.to_string(),
                response
                    .scopes
                    .iter()
                    .map(|scope| scope.scope_id.clone())
                    .collect(),
            ));
        }
    }
    let done = match action {
        StatefulMemoryScopeAction::List => None,
        StatefulMemoryScopeAction::Join => {
            Some("This thread now continues that investigation; its rules apply here.")
        }
        StatefulMemoryScopeAction::Leave => {
            Some("This thread no longer continues an investigation.")
        }
        StatefulMemoryScopeAction::End => {
            Some("Investigation ended: its rules no longer apply (they stay in history).")
        }
    };
    let mut lines: Vec<ratatui::text::Line<'static>> = Vec::new();
    if let Some(done) = done {
        lines.push(done.into());
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
        lines.push(format!("  {}. {} ({state})", index + 1, preview(&scope.title)).into());
        if let Some(condition) = &scope.end_condition {
            lines.push(format!("     ends: {condition}").into());
        }
    }
    Ok(PlainHistoryCell::new(lines))
}

/// The rows of `listing` numbered from `first` on, leaving out forgotten ones.
fn numbered(listing: &Listing, first: usize) -> Vec<(usize, StatefulMemoryItem)> {
    listing
        .slots
        .iter()
        .enumerate()
        .skip(first - 1)
        .filter(|(_, slot)| !slot.forgotten)
        .map(|(index, slot)| (index + 1, slot.item.clone()))
        .collect()
}

fn footer(listing: &Listing) -> Footer {
    if listing.cursor.is_some() {
        Footer::More
    } else {
        Footer::Complete
    }
}

fn request_id(action: &str) -> RequestId {
    RequestId::String(format!(
        "stateful-tui-memory-{action}-{}",
        uuid::Uuid::new_v4()
    ))
}

#[cfg(test)]
#[path = "stateful_memory_commands_tests.rs"]
mod tests;
