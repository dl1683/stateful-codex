//! `/memory` commands: list, page, add, forget and correct project memory with no model turn.
//!
//! List numbers are display conveniences. Mutations carry explicit entry IDs/revisions and
//! never resolve a number from an asynchronously refreshed list. Every result names the
//! thread and listing it belongs to, so a reply
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
use codex_app_server_protocol::StatefulMemoryApplyCategory;
use codex_app_server_protocol::StatefulMemoryCorrectParams;
use codex_app_server_protocol::StatefulMemoryCorrectResponse;
use codex_app_server_protocol::StatefulMemoryForgetParams;
use codex_app_server_protocol::StatefulMemoryForgetResponse;
use codex_app_server_protocol::StatefulMemoryItem;
use codex_app_server_protocol::StatefulMemoryReadParams;
use codex_app_server_protocol::StatefulMemoryReadResponse;
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
const USAGE: &str = "Usage: /memory, /memory next, /memory add <rule|about-me|decision|note> <text>, /memory forget <ID@REV>, /memory correct <ID@REV> <new text>, /memory apply <ID@REV> <rule|decision|ruled-out>, /memory undo <receipt-id>, /memory help";

/// What the user asked `/memory` to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemoryCommand {
    List,
    More,
    Help,
    Add(Addition),
    Forget(EntryTarget),
    Correct(EntryTarget, String),
    /// Apply a kept proposal as the user's own rule, decision or ruled-out approach.
    Apply(EntryTarget, StatefulMemoryApplyCategory),
    /// Undo one receipt by its exact ID.
    Undo(String),
}

/// An entry the user adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Addition {
    pub(crate) kind: StatefulMemoryAddKind,
    pub(crate) content: String,
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
        "add" => return parse_addition(rest).map(MemoryCommand::Add),
        _ => {}
    }
    let (target, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    match verb.to_ascii_lowercase().as_str() {
        "forget" if text.trim().is_empty() => Ok(MemoryCommand::Forget(entry_target(target)?)),
        "undo" if !target.is_empty() && text.trim().is_empty() => {
            Ok(MemoryCommand::Undo(target.to_string()))
        }
        "apply" => {
            let category = crate::stateful_memory_receipts::apply_category(text.trim())
                .ok_or_else(|| USAGE.to_string())?;
            Ok(MemoryCommand::Apply(entry_target(target)?, category))
        }
        "correct" if !text.trim().is_empty() => Ok(MemoryCommand::Correct(
            entry_target(target)?,
            text.trim().to_string(),
        )),
        _ => Err(USAGE.to_string()),
    }
}

/// An explicit revision-bound entry identity, independent of every displayed list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryTarget {
    entry_id: String,
    revision: u64,
}

fn entry_target(value: &str) -> Result<EntryTarget, String> {
    let (id, revision) = value.rsplit_once('@').ok_or_else(|| USAGE.to_string())?;
    let revision = revision
        .parse::<u64>()
        .ok()
        .filter(|revision| *revision > 0)
        .ok_or_else(|| USAGE.to_string())?;
    if id.is_empty() {
        return Err(USAGE.to_string());
    }
    Ok(EntryTarget {
        entry_id: id.to_string(),
        revision,
    })
}

/// `rule <text>`, `about <text>`, `decision <choice> [because <reason>]`,
/// `note <text>`.
fn parse_addition(rest: &str) -> Result<Addition, String> {
    const ADD_USAGE: &str = "Usage: /memory add rule <text>, /memory add about-me <text>, /memory add decision <choice> because <reason>, /memory add note <text>";
    let (kind, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let text = text.trim();
    if text.is_empty() {
        return Err(ADD_USAGE.to_string());
    }
    let addition = |kind, content: &str, reason: Option<&str>| Addition {
        kind,
        content: content.trim().to_string(),
        reason: reason.map(|reason| reason.trim().to_string()),
    };
    match kind.to_ascii_lowercase().as_str() {
        "rule" => {
            let (first, _) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
            if first.to_lowercase().starts_with("for:") {
                return Err(ADD_USAGE.to_string());
            }
            if first.eq_ignore_ascii_case("for") {
                return Err("Scoped rules are unsupported; nothing was added.".to_string());
            }
            Ok(addition(
                StatefulMemoryAddKind::Rule,
                text,
                /*reason*/ None,
            ))
        }
        "about" | "about-me" | "background" | "me" => {
            Ok(addition(
                StatefulMemoryAddKind::Background,
                text,
                /*reason*/ None,
            ))
        }
        "decision" => Ok(match text.split_once(" because ") {
            Some((choice, reason)) => {
                addition(StatefulMemoryAddKind::Decision, choice, Some(reason))
            }
            None => addition(StatefulMemoryAddKind::Decision, text, /*reason*/ None),
        }),
        "note" => Ok(addition(
            StatefulMemoryAddKind::Note,
            text,
            /*reason*/ None,
        )),
        _ => Err(ADD_USAGE.to_string()),
    }
}

/// The help shown by `/memory help`.
pub(crate) const HELP: &[&str] = &[
    "/memory - list what this project remembers (no model turn)",
    "/memory next - show the next entries of a long list (/memory refresh lists again)",
    "/memory add rule <text> - add a rule in your words",
    "/memory add about-me <text> - add something about you",
    "/memory add decision <choice> because <reason> - add a decision with its reason",
    "/memory add note <text> - add anything else worth keeping",
    "/memory correct <ID@REV> <text> - replace an entry with your words",
    "/memory forget <ID@REV> - stop using an entry (it stays in history)",
    "/memory apply <ID@REV> <rule|decision|ruled-out> - make a kept proposal yours (shown on its receipt)",
    "/memory undo <receipt-id> - undo one receipt: what it saved stops applying",
    "List numbers are display conveniences; mutations require explicit IDs. Outside the TUI: codex memory --help",
];

/// One numbered row of a listing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Slot {
    pub(crate) item: StatefulMemoryItem,
}

/// The listing shown for one thread: its rows by number, and where paging continues.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Listing {
    pub(crate) thread_id: String,
    pub(crate) generation: u64,
    pub(crate) project_id: String,
    pub(crate) client_id: uuid::Uuid,
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
        self.slots
            .extend(items.into_iter().map(|item| Slot { item }));
        numbers
    }
}

/// The listings by thread; a result applies only to the listing it was made for.
#[derive(Clone, Debug, Default)]
pub(crate) struct MemoryListing {
    shown: Arc<Mutex<Option<Listing>>>,
    next_generation: Arc<Mutex<u64>>,
    /// The entry listing generation last put on screen, by thread.
    displayed: Arc<Mutex<Option<(String, u64)>>>,
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

    /// Records that the entry list of `generation` of `thread_id` is now on screen.
    pub(crate) fn displayed(&self, thread_id: &str, generation: u64) {
        if self.lock().as_ref().is_some_and(|listing| {
            listing.thread_id == thread_id && listing.generation == generation
        }) {
            *self
                .displayed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some((thread_id.to_string(), generation));
        }
    }

    /// Application-boundary fence for every result, including errors and mutation receipts.
    pub(crate) fn accepts(&self, operation: u64, thread_id: &str, generation: Option<u64>) -> bool {
        if *self
            .next_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            != operation
        {
            return false;
        }
        generation.is_none_or(|generation| {
            self.lock().as_ref().is_some_and(|listing| {
                listing.thread_id == thread_id && listing.generation == generation
            })
        })
    }

    /// The listing whose numbers are on screen for `thread_id`: a number never names an entry
    /// of a list the user has not seen yet.
    fn on_screen(
        &self,
        thread_id: &str,
        project_id: &str,
        client_id: uuid::Uuid,
    ) -> Result<Listing, String> {
        let shown = self.for_thread(thread_id)?;
        if shown.project_id != project_id || shown.client_id != client_id {
            return Err(
                "The project or connection changed; run /memory to list it again.".to_string(),
            );
        }
        let displayed = self
            .displayed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if displayed != Some((thread_id.to_string(), shown.generation)) {
            return Err(
                "The list on screen is out of date; run /memory to list it again.".to_string(),
            );
        }
        Ok(shown)
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
    project_id: String,
    client_id: uuid::Uuid,
    app_event_tx: AppEventSender,
) {
    let operation = listing.begin();
    let command = parse(&args);
    tokio::spawn(async move {
        let Some(thread) = thread_id else {
            app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(new_error_event(
                "Project memory needs a session; start or resume one first.".to_string(),
            ))));
            return;
        };
        let insert = |cell: PlainHistoryCell, listing_generation: Option<u64>| {
            app_event_tx.send(AppEvent::StatefulMemoryResult {
                thread_id: thread,
                project_id: project_id.clone(),
                client_id,
                operation,
                cell: Box::new(cell),
                listing_generation,
            });
        };
        let thread_id = thread.to_string();
        let command = match command {
            Ok(command) => command,
            Err(usage) => {
                insert(new_error_event(usage), /*listing_generation*/ None);
                return;
            }
        };
        let request = Requests {
            handle: &request_handle,
            thread_id: &thread_id,
            project_id: &project_id,
            client_id,
        };
        let shows = |result: Result<(PlainHistoryCell, u64), String>| match result {
            Ok((cell, generation)) => (Ok(cell), Some(generation)),
            Err(error) => (Err(error), None),
        };
        let (result, listing_generation) = match command {
            MemoryCommand::List => shows(list(&request, &listing, operation).await),
            MemoryCommand::More => shows(more(&request, &listing, operation).await),
            command => (other(&request, command).await, None),
        };
        insert(result.unwrap_or_else(new_error_event), listing_generation);
    });
}

/// Runs a command that shows no numbered list.
async fn other(request: &Requests<'_>, command: MemoryCommand) -> Result<PlainHistoryCell, String> {
    match command {
        MemoryCommand::Help => Ok(PlainHistoryCell::new(
            HELP.iter().map(|line| (*line).into()).collect(),
        )),
        MemoryCommand::Add(addition) => add(request, addition).await,
        MemoryCommand::Forget(target) => forget(request, target).await,
        MemoryCommand::Correct(target, text) => correct(request, target, text).await,
        MemoryCommand::Apply(target, category) => {
            crate::stateful_memory_receipts::apply(
                request.handle,
                request.thread_id,
                request.project_id,
                target.entry_id,
                target.revision,
                category,
            )
            .await
        }
        MemoryCommand::Undo(receipt_id) => {
            crate::stateful_memory_receipts::undo(
                request.handle,
                request.thread_id,
                request.project_id,
                receipt_id,
            )
            .await
        }
        MemoryCommand::List | MemoryCommand::More => Err(USAGE.to_string()),
    }
}

/// The app-server handle and the thread a command acts for.
struct Requests<'a> {
    handle: &'a AppServerRequestHandle,
    thread_id: &'a str,
    project_id: &'a str,
    client_id: uuid::Uuid,
}

impl Requests<'_> {
    async fn page(&self, cursor: Option<String>) -> Result<StatefulMemoryReadResponse, String> {
        self.handle
            .request_typed(ClientRequest::StatefulMemoryRead {
                request_id: request_id("read"),
                params: StatefulMemoryReadParams {
                    expected_project_id: self.project_id.to_string(),
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

async fn list(
    request: &Requests<'_>,
    listing: &MemoryListing,
    generation: u64,
) -> Result<(PlainHistoryCell, u64), String> {
    let page = request.page(/*cursor*/ None).await?;
    let run = request.run().await;
    let mut shown = Listing {
        thread_id: request.thread_id.to_string(),
        generation,
        project_id: request.project_id.to_string(),
        client_id: request.client_id,
        slots: Vec::new(),
        cursor: page.next_cursor,
    };
    shown.append(page.data);
    let lines = memory_lines(&numbered(&shown, 1), footer(&shown), run.as_ref());
    // A newer listing started meanwhile owns the numbers; this older one is not shown, so
    // no visible number can point at a different entry.
    {
        let latest = listing
            .next_generation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *latest != generation {
            return Err(
                "A newer /memory command replaced this one; run /memory to refresh.".to_string(),
            );
        }
        let mut current = listing.lock();
        let newer = current
            .as_ref()
            .is_some_and(|current| current.generation > generation);
        if newer {
            return Err(
                "A newer /memory list replaced this one; use its explicit IDs.".to_string(),
            );
        }
        *current = Some(shown);
    }
    Ok((PlainHistoryCell::new(lines), generation))
}

async fn more(
    request: &Requests<'_>,
    listing: &MemoryListing,
    operation: u64,
) -> Result<(PlainHistoryCell, u64), String> {
    let shown = listing.on_screen(request.thread_id, request.project_id, request.client_id)?;
    let Some(cursor) = shown.cursor.clone() else {
        return Err("That was the whole list.".to_string());
    };
    let page = request.page(Some(cursor)).await.map_err(|error| {
        format!("{error}. Memory changed since the list was shown; run /memory to list it again.")
    })?;
    let used = shown.cursor.clone();
    let latest = listing
        .next_generation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *latest != operation {
        return Err(
            "A newer /memory command replaced this page; run /memory to refresh.".to_string(),
        );
    }
    listing
        .with_current(request.thread_id, shown.generation, |current| {
            // Another /memory next already showed this page: nothing is numbered twice.
            if current.cursor != used {
                return vec!["That page is already shown above.".into()];
            }
            let first = current.slots.len() + 1;
            current.cursor = page.next_cursor;
            current.append(page.data);
            memory_lines(
                &numbered(current, first),
                footer(current),
                /*run*/ None,
            )
        })
        .map(|lines| (PlainHistoryCell::new(lines), shown.generation))
        .ok_or_else(|| "A newer /memory list replaced this one; use its explicit IDs.".to_string())
}

async fn add(request: &Requests<'_>, addition: Addition) -> Result<PlainHistoryCell, String> {
    let response: StatefulMemoryAddResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryAdd {
            request_id: request_id("add"),
            params: StatefulMemoryAddParams {
                expected_project_id: request.project_id.to_string(),
                thread_id: request.thread_id.to_string(),
                kind: addition.kind,
                content: addition.content,
                scope: None,
                reason: addition.reason,
                client_action_id: uuid::Uuid::new_v4().to_string(),
                background_section: true,
            },
        })
        .await
        .map_err(|error| format!("Nothing was added: {error}"))?;
    let item = response.item;
    let what = match response.outcome {
        StatefulMemoryAddOutcome::Added => format!("Added {}", section_noun(item.section)),
        StatefulMemoryAddOutcome::AlreadyPresent | StatefulMemoryAddOutcome::AlreadyDone => {
            format!("Already saved as {}", section_noun(item.section))
        }
    };
    let hint = Some("Run /memory refresh to update the list.".to_string());
    Ok(new_info_event(
        format!("{what}: {}", preview(&item.content)),
        hint,
    ))
}

async fn forget(request: &Requests<'_>, target: EntryTarget) -> Result<PlainHistoryCell, String> {
    let _: StatefulMemoryForgetResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryForget {
            request_id: request_id("forget"),
            params: StatefulMemoryForgetParams {
                expected_project_id: request.project_id.to_string(),
                thread_id: request.thread_id.to_string(),
                entry_id: target.entry_id.clone(),
                expected_revision: target.revision,
            },
        })
        .await
        .map_err(|error| format!("Nothing was forgotten: {error}"))?;
    Ok(new_info_event(
        format!("Forgot {}@{}", target.entry_id, target.revision),
        Some(
            "It stays in history but no longer applies. Run /memory refresh to update the list."
                .to_string(),
        ),
    ))
}

async fn correct(
    request: &Requests<'_>,
    target: EntryTarget,
    text: String,
) -> Result<PlainHistoryCell, String> {
    let response: StatefulMemoryCorrectResponse = request
        .handle
        .request_typed(ClientRequest::StatefulMemoryCorrect {
            request_id: request_id("correct"),
            params: StatefulMemoryCorrectParams {
                expected_project_id: request.project_id.to_string(),
                thread_id: request.thread_id.to_string(),
                entry_id: target.entry_id,
                expected_revision: target.revision,
                content: text,
                background_section: true,
            },
        })
        .await
        .map_err(|error| format!("Nothing was corrected: {error}"))?;
    let item = response.item;
    Ok(new_info_event(
        format!(
            "Corrected {}@{}: {}",
            item.entry_id,
            item.revision,
            preview(&item.content)
        ),
        Some("Run /memory refresh to update the list.".to_string()),
    ))
}

fn numbered(listing: &Listing, first: usize) -> Vec<(usize, StatefulMemoryItem)> {
    listing
        .slots
        .iter()
        .enumerate()
        .skip(first - 1)
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
