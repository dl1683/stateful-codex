//! `codex memory` (and `codex exec memory`): list, page, add, correct and forget project memory
//! for a thread with no model turn, through the embedded app-server with the user's
//! authority. Mutations name an entry by its full ID and the revision the user saw, so a
//! later command never changes an entry other than the one listed.

use clap::Args;
use clap::Subcommand;
use codex_app_server_client::InProcessAppServerClient;
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
use codex_app_server_protocol::StatefulMemorySection;

/// Arguments of `memory`.
#[derive(Args, Debug, Clone)]
pub struct MemoryArgs {
    /// The thread whose project memory to use (its ID, as `codex resume` shows it).
    #[arg(long = "thread", value_name = "THREAD_ID")]
    pub thread_id: String,

    #[command(subcommand)]
    pub action: MemoryAction,
}

/// What to do with project memory.
#[derive(Subcommand, Debug, Clone)]
pub enum MemoryAction {
    /// List what the project remembers (50 entries a page).
    List {
        /// Continue a previous listing from the cursor it printed.
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Add an entry in your own words.
    Add {
        /// rule, about-me, decision or note.
        #[arg(value_parser = parse_kind)]
        kind: StatefulMemoryAddKind,
        /// For a rule: where it applies, in your words ("this investigation, until we agree").
        #[arg(long)]
        scope: Option<String>,
        /// For a decision: why it was made.
        #[arg(long)]
        reason: Option<String>,
        /// The words to save (put them after `--` if they start with a dash).
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Replace an entry with your words: ENTRY_ID@REVISION as `list` printed it.
    Correct {
        target: String,
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// Stop using an entry (it stays in history): ENTRY_ID@REVISION as `list` printed it.
    Forget { target: String },
}

fn parse_kind(value: &str) -> Result<StatefulMemoryAddKind, String> {
    match value.to_ascii_lowercase().as_str() {
        "rule" => Ok(StatefulMemoryAddKind::Rule),
        "about-me" | "about" | "background" => Ok(StatefulMemoryAddKind::Background),
        "decision" => Ok(StatefulMemoryAddKind::Decision),
        "note" => Ok(StatefulMemoryAddKind::Note),
        other => Err(format!(
            "unknown kind {other}; use rule, about-me, decision or note"
        )),
    }
}

/// Runs one memory command and prints its result.
pub(crate) async fn run(client: &InProcessAppServerClient, args: MemoryArgs) -> anyhow::Result<()> {
    let MemoryArgs { thread_id, action } = args;
    let request_id =
        |action: &str| RequestId::String(format!("codex-memory-{action}-{}", uuid::Uuid::new_v4()));
    match action {
        MemoryAction::List { cursor } => {
            let page: StatefulMemoryReadResponse = client
                .request_typed(ClientRequest::StatefulMemoryRead {
                    request_id: request_id("read"),
                    params: StatefulMemoryReadParams {
                        thread_id: thread_id.clone(),
                        cursor,
                        limit: Some(50),
                        background_section: true,
                    },
                })
                .await
                .map_err(|error| anyhow::anyhow!("could not read project memory: {error}"))?;
            print!("{}", listing(&page.data));
            match page.next_cursor {
                Some(cursor) => println!(
                    "More entries follow: codex memory --thread {thread_id} list --cursor {cursor}"
                ),
                None if page.data.is_empty() => println!("Nothing saved yet."),
                None => {}
            }
        }
        MemoryAction::Add {
            kind,
            scope,
            reason,
            text,
        } => {
            let response: StatefulMemoryAddResponse = client
                .request_typed(ClientRequest::StatefulMemoryAdd {
                    request_id: request_id("add"),
                    params: StatefulMemoryAddParams {
                        thread_id,
                        kind,
                        content: text.join(" "),
                        scope,
                        reason,
                        client_action_id: uuid::Uuid::new_v4().to_string(),
                        background_section: true,
                    },
                })
                .await
                .map_err(|error| anyhow::anyhow!("nothing was added: {error}"))?;
            let what = match response.outcome {
                StatefulMemoryAddOutcome::Added => "Added",
                StatefulMemoryAddOutcome::AlreadyPresent
                | StatefulMemoryAddOutcome::AlreadyDone => "Already saved",
            };
            println!(
                "{what} {}: {}",
                section_noun(response.item.section),
                response.item.content
            );
            println!("  {}", target(&response.item));
        }
        MemoryAction::Correct { target: aim, text } => {
            let (entry_id, expected_revision) = parse_target(&aim)?;
            let response: StatefulMemoryCorrectResponse = client
                .request_typed(ClientRequest::StatefulMemoryCorrect {
                    request_id: request_id("correct"),
                    params: StatefulMemoryCorrectParams {
                        thread_id,
                        entry_id,
                        expected_revision,
                        content: text.join(" "),
                        background_section: true,
                    },
                })
                .await
                .map_err(|error| anyhow::anyhow!("nothing was corrected: {error}"))?;
            println!(
                "Corrected {}: {}",
                section_noun(response.item.section),
                response.item.content
            );
            println!("  now {}", target(&response.item));
        }
        MemoryAction::Forget { target: aim } => {
            let (entry_id, expected_revision) = parse_target(&aim)?;
            let response: StatefulMemoryForgetResponse = client
                .request_typed(ClientRequest::StatefulMemoryForget {
                    request_id: request_id("forget"),
                    params: StatefulMemoryForgetParams {
                        thread_id,
                        entry_id,
                        expected_revision,
                    },
                })
                .await
                .map_err(|error| anyhow::anyhow!("nothing was forgotten: {error}"))?;
            println!(
                "Forgot {}@{}. It stays in history but no longer applies.",
                response.entry_id, response.revision
            );
        }
    }
    Ok(())
}

/// The listing, grouped by section, each entry with the target to correct or forget it.
pub(crate) fn listing(items: &[StatefulMemoryItem]) -> String {
    let sections = [
        (StatefulMemorySection::UserRule, "Your rules (applied)"),
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
    let mut out = String::new();
    for (section, title) in sections {
        let entries = items
            .iter()
            .filter(|item| item.section == section)
            .collect::<Vec<_>>();
        if entries.is_empty() {
            continue;
        }
        out.push_str(&format!("{title}\n"));
        for item in entries {
            let content = item
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("  - {content}\n    {}\n", target(item)));
            if let Some(scope) = &item.scope_title {
                out.push_str(&format!("    only in the investigation: {scope}\n"));
            }
        }
        out.push('\n');
    }
    out
}

fn target(item: &StatefulMemoryItem) -> String {
    format!("{}@{}", item.entry_id, item.revision)
}

fn parse_target(target: &str) -> anyhow::Result<(String, u64)> {
    let (entry_id, revision) = target.rsplit_once('@').ok_or_else(|| {
        anyhow::anyhow!("name the entry as ENTRY_ID@REVISION, exactly as `list` printed it")
    })?;
    let revision = revision
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("the revision after @ must be a number"))?;
    Ok((entry_id.to_string(), revision))
}

fn section_noun(section: StatefulMemorySection) -> &'static str {
    match section {
        StatefulMemorySection::UserRule => "rule (applied)",
        StatefulMemorySection::PendingRule => "task-limited rule (not applied)",
        StatefulMemorySection::UnverifiedRule => "rule (not applied)",
        StatefulMemorySection::Background => "note about you",
        StatefulMemorySection::Decision => "decision",
        StatefulMemorySection::Knowledge => "entry",
    }
}

#[cfg(test)]
#[path = "memory_command_tests.rs"]
mod tests;
