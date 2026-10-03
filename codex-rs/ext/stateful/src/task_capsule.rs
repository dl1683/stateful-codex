//! The task capsule of a continuation window: a bounded, host-built view of where the work
//! stood when compaction closed the previous window. It sits beside the native compaction
//! summary and is built only from host observations (the window journal and project files on
//! this host at the boundary) and the agent's own recorded words. It never invents a next step
//! and never turns an exit code into a test result.
//!
//! The capsule is captured once per window, so every step, retry and same-thread resume shows
//! the same text. Later commands or patches do not rewrite it; they add one short notice that
//! it may be historical.

use std::path::PathBuf;

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::TaskCapsule;
use codex_stateful_runtime::WindowEvent;
use codex_stateful_runtime::WindowEventKind;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::capsule_files::FileObserver;
use crate::capsule_files::Observation;
use crate::capsule_files::fingerprint;
use crate::window_capture::command_text;
use crate::window_capture::exit_code;
use crate::window_capture::exit_text;
use crate::window_capture::head;
use crate::window_capture::tail;
use crate::window_capture::validation_runner;

const WORLD_STATE_ID: &str = "stateful_task_capsule";
/// Stored for a window whose closed predecessor observed no work, so it is not rebuilt.
const NO_CAPSULE: &str = "(no work observed)";
const START_MARKER: &str = "<stateful_task_capsule>";
const END_MARKER: &str = "</stateful_task_capsule>";
const UPDATE_START_MARKER: &str = "<stateful_task_capsule_update>";
const UPDATE_END_MARKER: &str = "</stateful_task_capsule_update>";
/// The in-session spec's caps, markers included: 1,024 tokens and 4,096 bytes, or 512 tokens
/// and 2,048 bytes for models that compact at 80k tokens or less.
const LARGE_CAPS: CapsuleCaps = CapsuleCaps {
    bytes: 4_096,
    tokens: 1_024,
};
const SMALL_CAPS: CapsuleCaps = CapsuleCaps {
    bytes: 2_048,
    tokens: 512,
};
const SMALL_WINDOW_TOKENS: i64 = 80_000;
const MAX_FILES: usize = 8;
const MAX_PROGRESS_DOCS: usize = 2;
const PROGRESS_LINES: usize = 40;
const MAX_PROGRESS_BYTES: usize = 600;
const MAX_OUTPUT_TAIL_BYTES: usize = 320;
const MAX_QUOTE_BYTES: usize = 320;
const MAX_CLOSINGS: usize = 3;
const MAX_CLOSING_BYTES: usize = 240;
const MAX_WORKING_COMMAND_BYTES: usize = 480;
const MAX_OTHER_COMMAND_BYTES: usize = 200;
const MAX_CWD_BYTES: usize = 160;
/// Observations of the newest page the capsule reads (newest first).
const CAPSULE_PAGE: u32 = 64;
const PROGRESS_NAMES: &[&str] = &[
    "WORKLOG", "PROGRESS", "STATUS", "TODO", "NOTES", "PLAN", "RESULT", "JOURNAL", "HANDOFF",
];

/// A capsule's byte and token caps, markers included.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapsuleCaps {
    bytes: usize,
    tokens: usize,
}

/// The capsule caps for a model, from its automatic compaction limit.
pub(crate) fn capsule_caps(auto_compact_token_limit: Option<i64>) -> CapsuleCaps {
    match auto_compact_token_limit {
        Some(limit) if limit <= SMALL_WINDOW_TOKENS => SMALL_CAPS,
        Some(_) | None => LARGE_CAPS,
    }
}

/// A conservative token count without a tokenizer: one token per three bytes of ASCII and one
/// per non-ASCII character, which overestimates ordinary prose, code and paths (about four
/// bytes per token). It is an estimate, not a proof for adversarial text.
fn estimated_tokens(text: &str) -> usize {
    let ascii = text.bytes().filter(u8::is_ascii).count();
    let other = text
        .chars()
        .filter(|character| !character.is_ascii())
        .count();
    ascii.div_ceil(3) + other
}

fn fits(text: &str, caps: CapsuleCaps) -> bool {
    let wrapped = START_MARKER.len() + text.len() + END_MARKER.len();
    let wrapped_tokens =
        estimated_tokens(START_MARKER) + estimated_tokens(text) + estimated_tokens(END_MARKER);
    wrapped <= caps.bytes && wrapped_tokens <= caps.tokens
}

/// The window's capsule: the stored one, or one captured now from the journal. `None` when the
/// thread has journaled nothing yet.
pub(crate) async fn window_capsule(
    store: &StatefulRunStore,
    identity: (&str, &str, &str),
    roots: Vec<PathBuf>,
    caps: CapsuleCaps,
) -> Option<TaskCapsule> {
    let (thread_id, project_id, window_id) = identity;
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
        build_capsule(&events, through_seq, roots, caps).await
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

/// The newest page of the journal plus dedicated pages for what the capsule must not lose to
/// page overflow: working test runs, test runs, turns' closing messages, the newest plan and
/// commentary, and user messages.
async fn capsule_events(
    store: &StatefulRunStore,
    thread_id: &str,
    project_id: &str,
    through_seq: u64,
) -> Result<Vec<WindowEvent>, StatefulRunStoreError> {
    let mut events = store
        .window_events_newest_first(thread_id, project_id, 0, through_seq, CAPSULE_PAGE)
        .await?;
    let mut pages = Vec::new();
    for (kind, flag, limit) in [
        (WindowEventKind::Command, "working", 1),
        (WindowEventKind::Command, "validation", 16),
        (WindowEventKind::Message, "closing", 12),
    ] {
        pages.extend(
            store
                .window_events_flagged(thread_id, project_id, kind, flag, through_seq, limit)
                .await?,
        );
    }
    for (kind, limit) in [
        (WindowEventKind::Plan, 1),
        (WindowEventKind::Message, 4),
        (WindowEventKind::User, 16),
        (WindowEventKind::Command, 16),
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

/// The capsule section. `stale` reports commands or patches after the capsule's capture.
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
                        "Commands ran or patches applied after the task capsule above was captured: its file hashes, progress notes and test results may be historical.",
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

/// Builds the capsule within its caps. Lines are dropped from the lowest priority up
/// (progress notes, older files, output tails, files, failures), then the text is cut; the
/// next step, the working command and the turns' closing words are kept longest.
async fn build_capsule(
    newest_first: &[WindowEvent],
    through_seq: u64,
    roots: Vec<PathBuf>,
    caps: CapsuleCaps,
) -> String {
    let mut lines = lines(newest_first, through_seq, roots).await;
    let joined = |lines: &[(u8, String)]| {
        lines
            .iter()
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    while !fits(&joined(&lines), caps) {
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
    let mut text = joined(&lines);
    while !fits(&text, caps) && !text.is_empty() {
        text = head(&text, text.len().saturating_sub(64));
    }
    if text.len() < joined(&lines).len() {
        text.push_str(" [shortened]");
        while !fits(&text, caps) {
            text = format!("{} [shortened]", head(&text, text.len().saturating_sub(80)));
        }
    }
    text
}

/// Capsule lines in display order, each with the priority it is kept at (0 is kept longest).
async fn lines(
    newest_first: &[WindowEvent],
    through_seq: u64,
    roots: Vec<PathBuf>,
) -> Vec<(u8, String)> {
    let commands = || {
        newest_first
            .iter()
            .filter(|event| event.event.kind == WindowEventKind::Command)
    };
    let patched_after = |seq: u64| {
        newest_first.iter().any(|event| {
            event.event.kind == WindowEventKind::Edit
                && event.seq > seq
                && event.event.payload["status"] != "declined"
        })
    };
    let mut lines = vec![
        (
            0,
            format!(
                "Task capsule: where the work stood at compaction (host observations through journal event {through_seq} and the agent's own words; not a verified summary)."
            ),
        ),
        (0, next_step(newest_first, through_seq)),
    ];
    let working = commands().find(|event| event.event.payload["working"] == true);
    if let Some(working) = working {
        lines.push((0, working_line(working, patched_after(working.seq))));
        let runner = validation_runner(command_text(working));
        if let Some(earlier) = commands().find(|event| {
            event.seq < working.seq
                && exit_code(event).is_some_and(|code| code != 0)
                && validation_runner(command_text(event)) == runner
                && command_text(event) != command_text(working)
        }) {
            lines.push((
                2,
                format!(
                    "  An earlier, differently written run of the same runner ({}): `{}`.",
                    exit_text(earlier),
                    head(command_text(earlier), MAX_OTHER_COMMAND_BYTES)
                ),
            ));
        }
    }
    let closings = closings(newest_first, through_seq);
    if !closings.is_empty() {
        lines.push((
            0,
            "Closing words of recent turns (the agent's own, newest first):".to_string(),
        ));
        lines.extend(closings.into_iter().map(|closing| (0, closing)));
    }
    let validation = commands().find(|event| {
        event.event.payload["validation"] == true
            && working.is_none_or(|working| working.seq != event.seq)
    });
    let failure = commands().find(|event| {
        event.event.payload["status"] != "declined"
            && exit_code(event) != Some(0)
            && validation.is_none_or(|validation| validation.seq != event.seq)
    });
    for (label, event) in [
        ("Last test or check command", validation),
        ("Latest command without exit code 0", failure),
    ] {
        let Some(event) = event else {
            continue;
        };
        lines.push((
            3,
            format!(
                "{label} ({}{}): `{}`{}.",
                exit_text(event),
                if exit_code(event) == Some(0) {
                    ", a process exit code, not a test count"
                } else {
                    ""
                },
                head(command_text(event), MAX_OTHER_COMMAND_BYTES),
                if patched_after(event.seq) {
                    "; files were patched after it, so it may no longer hold"
                } else {
                    ""
                }
            ),
        ));
        let output = output_tail(event);
        if !output.is_empty() {
            lines.push((6, output));
        }
    }
    let mut observer = FileObserver::new(roots);
    let (files, more_files) = edited_files(newest_first, &mut observer).await;
    if !files.is_empty() {
        lines.push((
            5,
            format!(
                "Paths in recent patches, newest first, with the patch's overall outcome (which files a failed patch changed was not observed; scripts may have changed others):{}",
                if more_files > 0 {
                    format!(" {more_files} more not listed.")
                } else {
                    String::new()
                }
            ),
        ));
        for (index, (_, line, _)) in files.iter().enumerate() {
            lines.push((if index < 3 { 5 } else { 7 }, line.clone()));
        }
    }
    lines.extend(progress_notes(&files).into_iter().map(|note| (8, note)));
    lines.push((
        4,
        "Older detail is in the thread's history and in the published window receipts (memory_read). Read a file again before editing it if it may have changed.".to_string(),
    ));
    lines
}

fn working_line(working: &WindowEvent, patched_after: bool) -> String {
    let complete = working.event.payload["commandComplete"] != false;
    format!(
        "Last working test or check command (exit 0, a process exit code, not a test count){}{}: `{}` in {}.{}",
        working.event.payload["shell"]
            .as_str()
            .map_or_else(String::new, |shell| format!(", run by {shell}")),
        if complete {
            "; reuse it verbatim, with its directory and any environment settings in it"
        } else {
            "; shortened or redacted here, so not replayable as shown"
        },
        head(command_text(working), MAX_WORKING_COMMAND_BYTES),
        head(
            working.event.payload["cwd"]
                .as_str()
                .unwrap_or("an unrecorded directory"),
            MAX_CWD_BYTES
        ),
        if patched_after {
            " Files were patched after it."
        } else {
            ""
        }
    )
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

/// The next step only from what the agent itself recorded, or unknown. The newest plan
/// supersedes older plans even when all its steps are done.
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
    let newest_plan = newest_first
        .iter()
        .find(|event| event.event.kind == WindowEventKind::Plan);
    let open_step = newest_plan.and_then(|plan| {
        let steps = plan.event.payload["steps"].as_array()?;
        let step = steps
            .iter()
            .find(|step| step["status"] == "in_progress")
            .or_else(|| steps.iter().find(|step| step["status"] == "pending"))?;
        Some((plan.seq, step["step"].as_str()?.to_string()))
    });
    if let Some((seq, step)) = open_step {
        return format!(
            "Next step (the agent's last update_plan, {} observations before compaction): \"{}\".{}",
            through_seq.saturating_sub(seq),
            head(&step, MAX_QUOTE_BYTES),
            caveat(seq)
        );
    }
    let completed_plan = newest_plan.map(|plan| plan.seq);
    let intention = newest_first.iter().find(|event| {
        event.event.kind == WindowEventKind::Message
            && event.event.payload["phase"] == "commentary"
            && completed_plan.is_none_or(|plan| event.seq > plan)
    });
    if let Some(event) = intention {
        return format!(
            "Next step: no open plan step. Last announced intention (the agent's own words, {} observations before compaction): \"{}\".{}",
            through_seq.saturating_sub(event.seq),
            head(
                event.event.payload["text"].as_str().unwrap_or_default(),
                MAX_QUOTE_BYTES
            ),
            caveat(event.seq)
        );
    }
    if completed_plan.is_some() {
        return "Next step: unknown. The agent's last update_plan marked every step done; check the closing words below and the user's last message.".to_string();
    }
    "Next step: unknown. No plan or statement of intent was recorded; check the closing words below and the user's last message.".to_string()
}

fn output_tail(event: &WindowEvent) -> String {
    let output = event.event.payload["outputTail"]
        .as_str()
        .unwrap_or_default();
    if output.trim().is_empty() {
        return String::new();
    }
    let excerpt = tail(output, MAX_OUTPUT_TAIL_BYTES);
    format!(
        "  output, last {} of {} bytes: {}",
        excerpt.len(),
        event.event.payload["outputBytes"]
            .as_u64()
            .unwrap_or_default(),
        excerpt.replace('\n', " | ")
    )
}

/// Up to eight patched paths, newest first, with what this host saw at the boundary.
async fn edited_files(
    newest_first: &[WindowEvent],
    observer: &mut FileObserver,
) -> (Vec<(String, String, Option<Vec<u8>>)>, usize) {
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
        let (observed, bytes) = match observer.observe(&path).await {
            Observation::Read(bytes) => (
                format!("{} bytes, sha256 {}", bytes.len(), fingerprint(&bytes)),
                Some(bytes),
            ),
            Observation::NotRead(reason) => (reason.to_string(), None),
        };
        files.push((
            path.clone(),
            format!("- {path}: in a patch that {status}; {observed}"),
            bytes,
        ));
    }
    (files, more)
}

/// The last lines of up to two patched files named like progress notes, from the bytes already
/// read at the boundary.
fn progress_notes(files: &[(String, String, Option<Vec<u8>>)]) -> Vec<String> {
    let mut notes = Vec::new();
    for (path, _, bytes) in files {
        if notes.len() == MAX_PROGRESS_DOCS {
            break;
        }
        let Some(bytes) = bytes else {
            continue;
        };
        let name = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_default();
        if !PROGRESS_NAMES.iter().any(|marker| name.contains(marker)) {
            continue;
        }
        let text = String::from_utf8_lossy(bytes);
        let lines = text.lines().collect::<Vec<_>>();
        let first = lines.len().saturating_sub(PROGRESS_LINES);
        let excerpt = tail(&lines[first..].join("\n"), MAX_PROGRESS_BYTES);
        notes.push(format!(
            "Progress note {path} (chosen by its name), the end of its lines {}-{} of {} (sha256 {}); earlier sections are not shown:\n{excerpt}",
            first + 1,
            lines.len(),
            lines.len(),
            fingerprint(bytes),
        ));
    }
    notes
}

#[cfg(test)]
#[path = "task_capsule_tests.rs"]
mod tests;
