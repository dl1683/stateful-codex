//! The host journals its own observations of a thread's work as they complete: commands with
//! their exit codes, patch outcomes, the agent's plan updates and messages, and the user's
//! messages. Each is one committed journal row, so no model request is needed to remember it.
//!
//! Secrets are redacted from the source text before it is bounded, so a cut never splits a
//! credential away from the context that identifies it. Each observation also fits one
//! aggregate serialized budget: what does not fit is counted as omitted, never dropped silently.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_extension_api::ExtensionData;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolPayload;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::CommandExecutionStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::protocol::FileChange;
use codex_protocol::protocol::PatchApplyStatus;
use codex_protocol::user_input::UserInput;
use codex_stateful_runtime::NewWindowEvent;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WindowEvent;
use codex_stateful_runtime::WindowEventKind;
use serde_json::Value;
use serde_json::json;

const MAX_COMMAND_BYTES: usize = 1_024;
const MAX_CWD_BYTES: usize = 512;
const MAX_OUTPUT_TAIL_BYTES: usize = 2_048;
const MAX_MESSAGE_BYTES: usize = 1_024;
const MAX_USER_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 512;
const MAX_PLAN_STEPS: usize = 16;
const MAX_PLAN_STEP_BYTES: usize = 256;
/// Extra source text kept around a cut while redacting, so a secret's identifying prefix is
/// still present when the redactor runs.
const REDACTION_MARGIN_BYTES: usize = 1_024;
/// Serialized budget of one observation, well under the runtime's 16 KiB row bound even after
/// JSON escaping.
const MAX_OBSERVATION_BYTES: usize = 8 * 1_024;
const UPDATE_PLAN: &str = "update_plan";

/// The turn whose items are being journaled, set when the turn starts.
pub(crate) struct JournalTurn(pub(crate) String);

/// Records one completed item. A failure to journal is logged, never surfaced to the turn.
pub(crate) async fn journal_item(
    store: &StatefulRunStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    item: &TurnItem,
) {
    let Some((key, kind, payload)) = observation(item) else {
        return;
    };
    append(store, project_id, thread_id, turn_id, &key, kind, payload).await;
}

/// The update_plan arguments of calls in flight, kept until the call succeeds.
#[derive(Default)]
pub(crate) struct PendingPlans(Mutex<HashMap<String, Value>>);

/// Remembers an update_plan call's arguments; only a successful call is journaled.
pub(crate) fn remember_plan_call(
    turn_store: &ExtensionData,
    tool_name: &codex_extension_api::ToolName,
    call_id: &str,
    payload: &ToolPayload,
) {
    if !tool_name.is_default_namespace() || tool_name.name != UPDATE_PLAN {
        return;
    }
    let ToolPayload::Function { arguments } = payload else {
        return;
    };
    let Ok(arguments) = serde_json::from_str::<Value>(arguments) else {
        return;
    };
    turn_store
        .get_or_init(PendingPlans::default)
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(call_id.to_string(), plan_payload(&arguments));
}

/// Journals a remembered update_plan call once it has succeeded.
pub(crate) async fn journal_plan_call(
    store: &StatefulRunStore,
    turn_store: &ExtensionData,
    identity: (&str, &str, &str),
    call_id: &str,
    outcome: ToolCallOutcome,
) {
    let Some(plans) = turn_store.get::<PendingPlans>() else {
        return;
    };
    let Some(payload) = plans
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(call_id)
    else {
        return;
    };
    if !matches!(outcome, ToolCallOutcome::Completed { success: true }) {
        return;
    }
    let (project_id, thread_id, turn_id) = identity;
    let key = format!("{call_id}:plan");
    append(
        store,
        project_id,
        thread_id,
        turn_id,
        &key,
        WindowEventKind::Plan,
        payload,
    )
    .await;
}

async fn append(
    store: &StatefulRunStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    key: &str,
    kind: WindowEventKind,
    payload: Value,
) {
    let event = NewWindowEvent {
        thread_id: thread_id.to_string(),
        event_key: key.to_string(),
        project_id: project_id.to_string(),
        turn_id: turn_id.to_string(),
        kind,
        payload,
    };
    if let Err(error) = store.append_window_event(&event).await {
        tracing::warn!(%thread_id, event_key = %key, %error, "failed to journal a Stateful observation");
    }
}

fn observation(item: &TurnItem) -> Option<(String, WindowEventKind, Value)> {
    match item {
        TurnItem::CommandExecution(command) => {
            let status = match command.status {
                CommandExecutionStatus::InProgress => return None,
                CommandExecutionStatus::Completed => "completed",
                CommandExecutionStatus::Failed => "failed",
                CommandExecutionStatus::Declined => "declined",
            };
            let output_bytes = command.aggregated_output.as_ref().map_or_else(
                || {
                    command.stdout.as_deref().map_or(0, str::len)
                        + command.stderr.as_deref().map_or(0, str::len)
                },
                String::len,
            );
            let output_tail = match command.aggregated_output.as_deref() {
                Some(output) => clean_tail(output, MAX_OUTPUT_TAIL_BYTES),
                None => clean_tail(
                    &format!(
                        "{}{}",
                        tail(
                            command.stdout.as_deref().unwrap_or_default(),
                            MAX_OUTPUT_TAIL_BYTES + REDACTION_MARGIN_BYTES
                        ),
                        tail(
                            command.stderr.as_deref().unwrap_or_default(),
                            MAX_OUTPUT_TAIL_BYTES + REDACTION_MARGIN_BYTES
                        )
                    ),
                    MAX_OUTPUT_TAIL_BYTES,
                ),
            };
            Some((
                format!("{}:command", command.id),
                WindowEventKind::Command,
                json!({
                    "command": clean_head(&command.command.join(" "), MAX_COMMAND_BYTES),
                    "cwd": clean_head(&command.cwd.to_string(), MAX_CWD_BYTES),
                    "status": status,
                    "exitCode": command.exit_code,
                    "durationMs": command.duration.map(|duration| duration.as_millis() as u64),
                    "outputBytes": output_bytes,
                    "outputTail": output_tail,
                    // Whether the retained tail is all the output this item supplied.
                    "outputComplete": output_tail.len() == output_bytes,
                }),
            ))
        }
        TurnItem::FileChange(change) => {
            // One status covers the whole patch; per-file results are not observed.
            let status = match change.status {
                Some(PatchApplyStatus::Completed) => "applied",
                Some(PatchApplyStatus::Failed) => "failed",
                Some(PatchApplyStatus::Declined) => "declined",
                None => "unknown",
            };
            let mut entries = change
                .changes
                .iter()
                .map(|(path, change)| {
                    let (kind, moved_to) = match change {
                        FileChange::Add { .. } => ("add", None),
                        FileChange::Delete { .. } => ("delete", None),
                        FileChange::Update { move_path, .. } => ("update", move_path.as_ref()),
                    };
                    json!({
                        "path": clean_head(&path.display().to_string(), MAX_PATH_BYTES),
                        "change": kind,
                        "movedTo": moved_to.map(|path| clean_head(&path.display().to_string(), MAX_PATH_BYTES)),
                    })
                })
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
            let mut payload = json!({"status": status, "paths": [], "morePaths": 0});
            let mut omitted = 0usize;
            for entry in entries {
                let mut candidate = payload.clone();
                candidate["paths"]
                    .as_array_mut()
                    .unwrap_or_else(|| unreachable!("paths is an array"))
                    .push(entry);
                if serialized_len(&candidate) <= MAX_OBSERVATION_BYTES {
                    payload = candidate;
                } else {
                    omitted += 1;
                }
            }
            payload["morePaths"] = json!(omitted);
            Some((
                format!("{}:edit", change.id),
                WindowEventKind::Edit,
                payload,
            ))
        }
        TurnItem::AgentMessage(message) => {
            let text = message
                .content
                .iter()
                .map(|AgentMessageContent::Text { text }| text.as_str())
                .collect::<Vec<_>>()
                .join("");
            if text.trim().is_empty() {
                return None;
            }
            let phase = match message.phase {
                Some(MessagePhase::Commentary) => "commentary",
                Some(MessagePhase::FinalAnswer) => "final",
                None => "unknown",
            };
            Some((
                format!("{}:message", message.id),
                WindowEventKind::Message,
                json!({
                    "phase": phase,
                    "text": clean_head(&text, MAX_MESSAGE_BYTES),
                    "textBytes": text.len(),
                }),
            ))
        }
        TurnItem::UserMessage(message) => {
            let text = message
                .content
                .iter()
                .filter_map(|input| match input {
                    UserInput::Text { text, .. } => Some(text.as_str()),
                    // `UserInput` is non-exhaustive; only text is quoted.
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some((
                format!("{}:user", message.id),
                WindowEventKind::User,
                json!({"text": clean_head(&text, MAX_USER_BYTES), "textBytes": text.len()}),
            ))
        }
        _ => None,
    }
}

fn plan_payload(arguments: &Value) -> Value {
    let mut payload = json!({
        "explanation": arguments["explanation"]
            .as_str()
            .map(|text| clean_head(text, MAX_MESSAGE_BYTES)),
        "steps": [],
        "moreSteps": 0,
    });
    let steps = arguments["plan"].as_array().cloned().unwrap_or_default();
    let mut omitted = 0usize;
    for (index, step) in steps.iter().enumerate() {
        let entry = json!({
            "step": clean_head(step["step"].as_str().unwrap_or_default(), MAX_PLAN_STEP_BYTES),
            "status": head(step["status"].as_str().unwrap_or_default(), 32),
        });
        let mut candidate = payload.clone();
        candidate["steps"]
            .as_array_mut()
            .unwrap_or_else(|| unreachable!("steps is an array"))
            .push(entry);
        if index < MAX_PLAN_STEPS && serialized_len(&candidate) <= MAX_OBSERVATION_BYTES {
            payload = candidate;
        } else {
            omitted += 1;
        }
    }
    payload["moreSteps"] = json!(omitted);
    payload
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

/// The first `max` bytes of `text` after redacting a margin beyond the cut.
fn clean_head(text: &str, max: usize) -> String {
    head(
        &codex_secrets::redact_secrets(head(text, max + REDACTION_MARGIN_BYTES)),
        max,
    )
}

/// The last `max` bytes of `text` after redacting a margin before the cut.
fn clean_tail(text: &str, max: usize) -> String {
    tail(
        &codex_secrets::redact_secrets(tail(text, max + REDACTION_MARGIN_BYTES)),
        max,
    )
}

pub(crate) fn command_text(event: &WindowEvent) -> &str {
    event.event.payload["command"].as_str().unwrap_or_default()
}

pub(crate) fn exit_code(event: &WindowEvent) -> Option<i64> {
    event.event.payload["exitCode"].as_i64()
}

pub(crate) fn exit_text(event: &WindowEvent) -> String {
    match (event.event.payload["status"].as_str(), exit_code(event)) {
        (Some("declined"), _) => "declined (not run)".to_string(),
        (_, Some(code)) => format!("exit {code}"),
        (_, None) => "exit unknown".to_string(),
    }
}

/// Whether a command looks like a test or check runner. Recognition is by name only: it says
/// nothing about what passed, and a compound command's exit code is its last command's.
pub(crate) fn is_validation_command(command: &str) -> bool {
    validation_runner(command).is_some()
}

/// The test or check runner a command names (see [`is_validation_command`]).
pub(crate) fn validation_runner(command: &str) -> Option<&'static str> {
    const RUNNERS: &[&str] = &[
        "cargo test",
        "cargo nextest",
        "just test",
        "pytest",
        "unittest",
        "npm test",
        "npm run test",
        "pnpm test",
        "yarn test",
        "go test",
        "dotnet test",
        "mvn test",
        "gradle test",
        "make test",
        "ctest",
        "rspec",
        "jest",
        "vitest",
        "tox",
        "phpunit",
        "cargo check",
        "cargo clippy",
        "tsc",
    ];
    let command = command.to_ascii_lowercase();
    RUNNERS
        .iter()
        .find(|runner| command.contains(*runner))
        .copied()
}

/// The first `max` bytes of `text`, cut at a character boundary.
pub(crate) fn head(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// The last `max` bytes of `text`, cut at a character boundary.
pub(crate) fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

#[cfg(test)]
#[path = "window_capture_tests.rs"]
mod tests;
