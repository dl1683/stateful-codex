//! The task capsule of a continuation window: a bounded, host-built view of where the work
//! stood when compaction closed the previous window. It sits beside the native compaction
//! summary and is built only from host observations (the window journal and the files on disk
//! at the boundary) and the agent's own recorded words. It never invents a next step and never
//! turns an exit code into a test result.
//!
//! The capsule is captured once per window, so every step, retry and same-thread resume shows
//! the same text. Later edits do not rewrite it; they add one short notice that it is now
//! historical.

use std::path::Path;

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::TaskCapsule;
use codex_stateful_runtime::WindowEvent;
use codex_stateful_runtime::WindowEventKind;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::window_capture::command_text;
use crate::window_capture::exit_code;
use crate::window_capture::exit_text;
use crate::window_capture::head;
use crate::window_capture::is_validation_command;
use crate::window_capture::tail;
use crate::window_capture::validation_runner;

const WORLD_STATE_ID: &str = "stateful_task_capsule";
/// Stored for a window whose closed predecessor observed no work, so it is not rebuilt.
const NO_CAPSULE: &str = "(no work observed)";
const START_MARKER: &str = "<stateful_task_capsule>";
const END_MARKER: &str = "</stateful_task_capsule>";
const UPDATE_START_MARKER: &str = "<stateful_task_capsule_update>";
const UPDATE_END_MARKER: &str = "</stateful_task_capsule_update>";
/// Capsule bound, markers included (the in-session spec's 4,096-byte cap), and the smaller
/// bound for models that compact at 80k tokens or less.
const MAX_CAPSULE_BYTES: usize = 4_096;
const MAX_SMALL_WINDOW_CAPSULE_BYTES: usize = 2_048;
const SMALL_WINDOW_TOKENS: i64 = 80_000;
const MAX_FILES: usize = 8;
const MAX_PROGRESS_DOCS: usize = 2;
const PROGRESS_LINES: usize = 40;
const MAX_PROGRESS_BYTES: usize = 700;
const MAX_VALIDATION_TAIL_BYTES: usize = 480;
const MAX_FAILURE_TAIL_BYTES: usize = 320;
const MAX_QUOTE_BYTES: usize = 400;
/// Closing messages quoted, and the bytes of each quoted from its end.
const MAX_CLOSINGS: usize = 3;
const MAX_CLOSING_BYTES: usize = 320;
/// The working command is quoted verbatim, so it gets more room than other command lines.
const MAX_WORKING_COMMAND_BYTES: usize = 600;
/// Files larger than this are named but not hashed or read at the boundary.
const MAX_OBSERVED_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Observations the capsule is built from (newest first).
const CAPSULE_PAGE: u32 = 64;
const PROGRESS_NAMES: &[&str] = &[
    "WORKLOG", "PROGRESS", "STATUS", "TODO", "NOTES", "PLAN", "RESULT", "JOURNAL", "HANDOFF",
];

/// The capsule bound for a model, from its automatic compaction limit.
pub(crate) fn capsule_bytes(auto_compact_token_limit: Option<i64>) -> usize {
    match auto_compact_token_limit {
        Some(limit) if limit <= SMALL_WINDOW_TOKENS => MAX_SMALL_WINDOW_CAPSULE_BYTES,
        Some(_) | None => MAX_CAPSULE_BYTES,
    }
}

/// The window's capsule: the stored one, or one captured now from the journal. `None` when the
/// thread has journaled nothing yet.
pub(crate) async fn window_capsule(
    store: &StatefulRunStore,
    thread_id: &str,
    project_id: &str,
    window_id: &str,
    max_bytes: usize,
) -> Option<TaskCapsule> {
    match store.task_capsule(thread_id, window_id).await {
        Ok(Some(capsule)) => return Some(capsule),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%thread_id, %error, "failed to read a task capsule");
            return None;
        }
    }
    let through_seq = store
        .window_event_watermark(thread_id, project_id)
        .await
        .ok()?;
    if through_seq == 0 {
        return None;
    }
    let events = match capsule_events(store, thread_id, project_id, through_seq).await {
        Ok(events) => events,
        Err(error) => {
            tracing::warn!(%thread_id, %error, "failed to read the window journal");
            return None;
        }
    };
    // Only messages and no work: nothing a capsule would add to the summary.
    let observed_work = events.iter().any(|event| {
        matches!(
            event.event.kind,
            WindowEventKind::Edit | WindowEventKind::Command | WindowEventKind::Plan
        ) || (event.event.kind == WindowEventKind::Message
            && event.event.payload["phase"] == "commentary")
    });
    let body = if observed_work {
        build_capsule(&events, through_seq, max_bytes).await
    } else {
        NO_CAPSULE.to_string()
    };
    match store
        .capture_task_capsule(thread_id, window_id, &TaskCapsule { through_seq, body })
        .await
    {
        Ok(capsule) => Some(capsule),
        Err(error) => {
            tracing::warn!(%thread_id, %error, "failed to store a task capsule");
            None
        }
    }
}

/// Whether a stored capsule has anything to show.
pub(crate) fn has_content(capsule: &TaskCapsule) -> bool {
    capsule.body != NO_CAPSULE
}

/// The newest page of the journal plus the newest observations of each kind the capsule needs,
/// so a working command, a plan or a closing message older than the page still reaches it.
async fn capsule_events(
    store: &StatefulRunStore,
    thread_id: &str,
    project_id: &str,
    through_seq: u64,
) -> Result<Vec<WindowEvent>, codex_stateful_runtime::StatefulRunStoreError> {
    let mut events = store
        .window_events_newest_first(thread_id, project_id, 0, through_seq, CAPSULE_PAGE)
        .await?;
    let mut pages = store
        .window_validation_commands(thread_id, project_id, through_seq, 16)
        .await?;
    for (kind, limit) in [
        (WindowEventKind::Command, 16),
        (WindowEventKind::Message, 32),
        (WindowEventKind::Plan, 4),
        (WindowEventKind::User, 16),
    ] {
        pages.extend(
            store
                .window_events_of_kind(thread_id, project_id, kind, through_seq, limit)
                .await?,
        );
    }
    for event in pages {
        if !events.iter().any(|known| known.seq == event.seq) {
            events.push(event);
        }
    }
    events.sort_by(|left, right| right.seq.cmp(&left.seq));
    Ok(events)
}

/// The capsule section. `stale` reports journaled edits after the capsule's capture.
pub(crate) fn capsule_section(
    window_id: &str,
    capsule: &TaskCapsule,
    stale: bool,
) -> WorldStateSectionContribution {
    let snapshot = json!({
        "windowId": window_id,
        "digest": format!("{:x}", Sha256::digest(capsule.body.as_bytes())),
        "stale": stale,
    });
    let body = capsule.body.clone();
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot.clone(), move |previous| {
        match previous {
            PreviousWorldStateSection::Known(previous) if previous == &snapshot => None,
            PreviousWorldStateSection::Known(previous)
                if previous.get("digest") == snapshot.get("digest") =>
            {
                (snapshot.get("stale") == Some(&Value::Bool(true))).then(|| {
                    RenderedWorldStateFragment::new(
                        "developer",
                        (UPDATE_START_MARKER, UPDATE_END_MARKER),
                        "Files were changed after the task capsule above was captured: its file hashes, progress notes and validation status are now historical.",
                    )
                })
            }
            PreviousWorldStateSection::Absent
            | PreviousWorldStateSection::Unknown
            | PreviousWorldStateSection::Known(_) => Some(RenderedWorldStateFragment::new(
                "developer",
                (START_MARKER, END_MARKER),
                body.clone(),
            )),
        }
    })
}

struct Sections {
    header: String,
    next_step: String,
    validation: Option<(String, String)>,
    /// The last test or check command that exited 0, verbatim with its working directory, and
    /// the failing form of the same runner it replaced. Kept whatever else is trimmed.
    working: Option<String>,
    failure: Option<(String, String)>,
    /// The closing words of the most recent turns, quoted (a next step stated before a
    /// detour lives here).
    closings: Vec<String>,
    files: Vec<String>,
    more_files: usize,
    progress: Vec<String>,
    footer: String,
}

impl Sections {
    /// Lines in display order, each with the priority it is kept at (0 is kept longest).
    fn lines(&self) -> Vec<(u8, String)> {
        let mut lines = vec![(0, self.header.clone()), (0, self.next_step.clone())];
        if let Some(working) = &self.working {
            lines.push((0, working.clone()));
        }
        if !self.closings.is_empty() {
            lines.push((
                1,
                "Closing words of recent turns (the agent's own, newest first):".to_string(),
            ));
            for (index, closing) in self.closings.iter().enumerate() {
                lines.push((
                    1 + u8::try_from(index).unwrap_or(u8::MAX).min(2),
                    closing.clone(),
                ));
            }
        }
        for (line, output) in [&self.failure, &self.validation].into_iter().flatten() {
            lines.push((2, line.clone()));
            if !output.is_empty() {
                lines.push((5, output.clone()));
            }
        }
        if !self.files.is_empty() {
            lines.push((
                4,
                format!(
                    "Paths in recent patches, newest first, with the patch's overall outcome (which files a failed patch changed was not observed; scripts may have changed others):{}",
                    if self.more_files > 0 {
                        format!(" {} more not listed.", self.more_files)
                    } else {
                        String::new()
                    }
                ),
            ));
            for (index, file) in self.files.iter().enumerate() {
                lines.push((if index < 3 { 4 } else { 6 }, file.clone()));
            }
        }
        lines.extend(self.progress.iter().map(|note| (7, note.clone())));
        lines.push((3, self.footer.clone()));
        lines
    }
}

/// Builds the capsule text within `max_bytes` (markers included). Lines are dropped from the
/// lowest priority up (progress notes, older files, output tails, files, closing words) until
/// the rest fits; the next step and the working command are kept, cut if they must be.
async fn build_capsule(newest_first: &[WindowEvent], through_seq: u64, max_bytes: usize) -> String {
    let budget = max_bytes.saturating_sub(START_MARKER.len() + END_MARKER.len());
    let mut lines = sections(newest_first, through_seq).await.lines();
    let joined = |lines: &[(u8, String)]| {
        lines
            .iter()
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    while joined(&lines).len() > budget {
        let lowest = lines
            .iter()
            .map(|(priority, _)| *priority)
            .max()
            .unwrap_or(0);
        if lowest == 0 {
            break;
        }
        // Drop the last line at the lowest priority (older items are listed later).
        if let Some(index) = lines.iter().rposition(|(priority, _)| *priority == lowest) {
            lines.remove(index);
        }
    }
    let text = joined(&lines);
    if text.len() <= budget {
        return text;
    }
    let mut text = head(&text, budget.saturating_sub(40));
    text.push_str("\n[task capsule shortened]");
    text
}

async fn sections(newest_first: &[WindowEvent], through_seq: u64) -> Sections {
    let validation = newest_first.iter().find(|event| {
        event.event.kind == WindowEventKind::Command && is_validation_command(command_text(event))
    });
    let failure = newest_first.iter().find(|event| {
        event.event.kind == WindowEventKind::Command
            && event.event.payload["status"] != "declined"
            && exit_code(event) != Some(0)
            && validation.is_none_or(|validation| validation.seq != event.seq)
    });
    let edited_after = |seq: u64| {
        newest_first
            .iter()
            .any(|event| event.event.kind == WindowEventKind::Edit && event.seq > seq)
    };
    let working = newest_first.iter().find(|event| {
        event.event.kind == WindowEventKind::Command
            && event.event.payload["status"] == "completed"
            && exit_code(event) == Some(0)
            && is_validation_command(command_text(event))
    });
    let working_line = working.map(|working| {
        let runner = validation_runner(command_text(working));
        // A learned workaround: an earlier failing invocation of the same runner, written
        // differently from the form that then worked.
        let replaced = newest_first.iter().find(|event| {
            event.seq < working.seq
                && event.event.kind == WindowEventKind::Command
                && exit_code(event) != Some(0)
                && event.event.payload["status"] != "declined"
                && validation_runner(command_text(event)) == runner
                && command_text(event) != command_text(working)
        });
        format!(
            "Last working test or check command (exit 0, a process exit code, not a test count; reuse it verbatim, with its directory and any environment settings in it): `{}` in {}.{}{}",
            head(command_text(working), MAX_WORKING_COMMAND_BYTES),
            head(working.event.payload["cwd"].as_str().unwrap_or("an unrecorded directory"), 240),
            replaced.map_or_else(String::new, |failed| format!(
                " It replaced a form that failed: `{}` {}.",
                head(command_text(failed), 240),
                exit_text(failed)
            )),
            if edited_after(working.seq) {
                " Files were changed after it."
            } else {
                ""
            }
        )
    });
    // The newest test command is shown again only when it is not the working one.
    let validation =
        validation.filter(|event| working.is_none_or(|working| working.seq != event.seq));
    let validation = validation.map(|event| {
        (
            format!(
                "Last test or check command: `{}` {}{}{}.",
                head(command_text(event), 240),
                exit_text(event),
                if exit_code(event) == Some(0) {
                    " (the process exit code, not a test count)"
                } else {
                    ""
                },
                if edited_after(event.seq) {
                    "; files were changed after it, so it may no longer hold"
                } else {
                    ""
                }
            ),
            output_tail(event, MAX_VALIDATION_TAIL_BYTES),
        )
    });
    let failure = failure.map(|event| {
        (
            format!(
                "Latest command without exit code 0: `{}` {}.",
                head(command_text(event), 240),
                exit_text(event)
            ),
            output_tail(event, MAX_FAILURE_TAIL_BYTES),
        )
    });
    let (files, more_files) = edited_files(newest_first).await;
    Sections {
        header: format!(
            "Task capsule: where the work stood at compaction (host observations through journal event {through_seq} and the agent's own words; not a verified summary)."
        ),
        next_step: next_step(newest_first, through_seq),
        validation,
        working: working_line,
        failure,
        closings: closings(newest_first, through_seq),
        files: files.iter().map(|(_, line)| line.clone()).collect(),
        more_files,
        progress: progress_notes(&files).await,
        footer: "Older detail is in the thread's history and in the published window receipts (memory_read). Read a file again before editing it if it may have changed.".to_string(),
    }
}

/// The ends of the agent's most recent closing messages, one per turn, quoted exactly.
fn closings(newest_first: &[WindowEvent], through_seq: u64) -> Vec<String> {
    let mut turns = Vec::<&str>::new();
    let mut closings = Vec::new();
    for event in newest_first.iter().filter(|event| {
        event.event.kind == WindowEventKind::Message && event.event.payload["phase"] != "commentary"
    }) {
        if turns.contains(&event.event.turn_id.as_str()) {
            continue;
        }
        turns.push(&event.event.turn_id);
        let text = event.event.payload["tail"]
            .as_str()
            .or_else(|| event.event.payload["text"].as_str())
            .unwrap_or_default();
        closings.push(format!(
            "- {} observations before compaction: \"...{}\"",
            through_seq.saturating_sub(event.seq),
            tail(text, MAX_CLOSING_BYTES).replace('\n', " ")
        ));
        if closings.len() == MAX_CLOSINGS {
            break;
        }
    }
    closings
}

/// The next step only from what the agent itself recorded, or unknown.
fn next_step(newest_first: &[WindowEvent], through_seq: u64) -> String {
    let later_user_message = |seq: u64| {
        newest_first
            .iter()
            .any(|event| event.event.kind == WindowEventKind::User && event.seq > seq)
    };
    let caveat = |seq: u64| {
        if later_user_message(seq) {
            " A later user message may have changed it; the user's words win."
        } else {
            ""
        }
    };
    let plan = newest_first.iter().find_map(|event| {
        if event.event.kind != WindowEventKind::Plan {
            return None;
        }
        let steps = event.event.payload["steps"].as_array()?;
        let step = steps
            .iter()
            .find(|step| step["status"] == "in_progress")
            .or_else(|| steps.iter().find(|step| step["status"] == "pending"))?;
        Some((event.seq, step["step"].as_str()?.to_string()))
    });
    if let Some((seq, step)) = plan {
        return format!(
            "Next step (the agent's last update_plan, {} observations before compaction): \"{}\".{}",
            through_seq.saturating_sub(seq),
            head(&step, MAX_QUOTE_BYTES),
            caveat(seq)
        );
    }
    let intention = newest_first.iter().find(|event| {
        event.event.kind == WindowEventKind::Message && event.event.payload["phase"] == "commentary"
    });
    if let Some(event) = intention {
        return format!(
            "Next step: no plan was recorded. Last announced intention (the agent's own words, {} observations before compaction): \"{}\".{}",
            through_seq.saturating_sub(event.seq),
            head(
                event.event.payload["text"].as_str().unwrap_or_default(),
                MAX_QUOTE_BYTES
            ),
            caveat(event.seq)
        );
    }
    "Next step: unknown. No plan or statement of intent was recorded; check the latest failure and the user's last message.".to_string()
}

fn output_tail(event: &WindowEvent, max: usize) -> String {
    let output = event.event.payload["outputTail"]
        .as_str()
        .unwrap_or_default();
    if output.trim().is_empty() {
        return String::new();
    }
    let excerpt = tail(output, max);
    format!(
        "  output, last {} of {} bytes: {}",
        excerpt.len(),
        event.event.payload["outputBytes"]
            .as_u64()
            .unwrap_or_default(),
        excerpt.replace('\n', " | ")
    )
}

/// Up to eight edited paths, newest first, with what is on disk at the boundary.
async fn edited_files(newest_first: &[WindowEvent]) -> (Vec<(String, String)>, usize) {
    let mut seen = Vec::<(String, String)>::new();
    let mut more = 0;
    for event in newest_first
        .iter()
        .filter(|event| event.event.kind == WindowEventKind::Edit)
    {
        let status = event.event.payload["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        for path in event.event.payload["paths"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let path = path["movedTo"]
                .as_str()
                .or_else(|| path["path"].as_str())
                .unwrap_or_default()
                .to_string();
            if path.is_empty() || seen.iter().any(|(seen, _)| *seen == path) {
                continue;
            }
            if seen.len() == MAX_FILES {
                more += 1;
                continue;
            }
            seen.push((path, status.clone()));
        }
        more += event.event.payload["morePaths"]
            .as_u64()
            .unwrap_or_default() as usize;
    }
    let mut files = Vec::with_capacity(seen.len());
    for (path, status) in seen {
        let observed = observe_file(Path::new(&path)).await;
        files.push((
            path.clone(),
            format!("- {path}: in a patch that {status}; {observed}"),
        ));
    }
    (files, more)
}

async fn observe_file(path: &Path) -> String {
    if !path.is_absolute() {
        return "not observed here".to_string();
    }
    match tokio::fs::metadata(path).await {
        Ok(metadata) if !metadata.is_file() => "not a regular file".to_string(),
        Ok(metadata) if metadata.len() > MAX_OBSERVED_FILE_BYTES => {
            format!("{} bytes, too large to hash", metadata.len())
        }
        Ok(_) => match tokio::fs::read(path).await {
            Ok(bytes) => format!(
                "{} bytes, sha256 {}",
                bytes.len(),
                &format!("{:x}", Sha256::digest(&bytes))[..16]
            ),
            Err(_) => "unreadable here".to_string(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "missing at the boundary".to_string()
        }
        Err(_) => "unreadable here".to_string(),
    }
}

/// The last lines of up to two edited files named like progress notes.
async fn progress_notes(files: &[(String, String)]) -> Vec<String> {
    let mut notes = Vec::new();
    for (path, _) in files {
        if notes.len() == MAX_PROGRESS_DOCS {
            break;
        }
        let path = Path::new(path);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default();
        if !PROGRESS_NAMES.iter().any(|marker| name.contains(marker)) || !path.is_absolute() {
            continue;
        }
        let Ok(metadata) = tokio::fs::metadata(path).await else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_OBSERVED_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(path).await else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let lines = text.lines().collect::<Vec<_>>();
        let first = lines.len().saturating_sub(PROGRESS_LINES);
        let excerpt = tail(&lines[first..].join("\n"), MAX_PROGRESS_BYTES);
        notes.push(format!(
            "Progress note {} (chosen by its name), the end of its lines {}-{} of {} (sha256 {}); earlier sections are not shown:\n{}",
            path.display(),
            first + 1,
            lines.len(),
            lines.len(),
            &format!("{:x}", Sha256::digest(&bytes))[..16],
            excerpt
        ));
    }
    notes
}

#[cfg(test)]
#[path = "task_capsule_tests.rs"]
mod tests;
