//! The host journals its own observations of a thread's work as they complete (commands with
//! their exit codes, patch outcomes, the agent's plan updates and messages, the user's
//! messages) and publishes them to project memory once per context window, so the agent never
//! has to spend a model request to remember what it did.
//!
//! Publication is a host-written, unverified, unpromoted note per closed window: activity
//! receipts, not conclusions. It happens when compaction closes a window and, for windows a
//! session left open, when the next session of the project starts. No routine publication
//! happens per turn, so a keeper resuming the same thread adds nothing until a window closes.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use codex_extension_api::ExtensionData;
use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolPayload;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
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
use codex_stateful_runtime::WindowPublication;
use codex_stateful_runtime::WindowPublicationState;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

use crate::services::ProjectIntelligenceServices;

const MAX_COMMAND_BYTES: usize = 1_024;
const MAX_CWD_BYTES: usize = 512;
const MAX_OUTPUT_TAIL_BYTES: usize = 2_048;
const MAX_MESSAGE_BYTES: usize = 1_024;
const MAX_USER_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 512;
const MAX_PATHS: usize = 32;
const MAX_PLAN_STEPS: usize = 16;
const MAX_PLAN_STEP_BYTES: usize = 256;
/// Publication content stays under the blackboard's 4,096-byte entry bound.
const MAX_PUBLICATION_BYTES: usize = 3_800;
/// Observations read to build one publication.
const PUBLICATION_PAGE: u32 = 100;
const UPDATE_PLAN: &str = "update_plan";

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
    let payload = redacted(payload);
    append(store, project_id, thread_id, turn_id, &key, kind, payload).await;
}

/// The turn whose items are being journaled, set when the turn starts.
pub(crate) struct JournalTurn(pub(crate) String);

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
        .insert(call_id.to_string(), redacted(plan_payload(&arguments)));
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
            let output = command.aggregated_output.clone().unwrap_or_else(|| {
                format!(
                    "{}{}",
                    command.stdout.as_deref().unwrap_or_default(),
                    command.stderr.as_deref().unwrap_or_default()
                )
            });
            let tail = tail(&output, MAX_OUTPUT_TAIL_BYTES);
            Some((
                format!("{}:command", command.id),
                WindowEventKind::Command,
                json!({
                    "command": head(&command.command.join(" "), MAX_COMMAND_BYTES),
                    "cwd": head(&command.cwd.to_string(), MAX_CWD_BYTES),
                    "status": status,
                    "exitCode": command.exit_code,
                    "durationMs": command.duration.map(|duration| duration.as_millis() as u64),
                    "outputBytes": output.len(),
                    "outputTail": tail,
                    "outputComplete": tail.len() == output.len(),
                }),
            ))
        }
        TurnItem::FileChange(change) => {
            let mut paths = change
                .changes
                .iter()
                .map(|(path, change)| {
                    let (kind, moved_to) = match change {
                        FileChange::Add { .. } => ("add", None),
                        FileChange::Delete { .. } => ("delete", None),
                        FileChange::Update { move_path, .. } => ("update", move_path.as_ref()),
                    };
                    json!({
                        "path": head(&path.display().to_string(), MAX_PATH_BYTES),
                        "change": kind,
                        "movedTo": moved_to.map(|path| head(&path.display().to_string(), MAX_PATH_BYTES)),
                    })
                })
                .collect::<Vec<_>>();
            paths.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
            let more = paths.len().saturating_sub(MAX_PATHS);
            paths.truncate(MAX_PATHS);
            // One status covers the whole patch; per-file results are not observed.
            let status = match change.status {
                Some(PatchApplyStatus::Completed) => "applied",
                Some(PatchApplyStatus::Failed) => "failed",
                Some(PatchApplyStatus::Declined) => "declined",
                None => "unknown",
            };
            Some((
                format!("{}:edit", change.id),
                WindowEventKind::Edit,
                json!({"status": status, "paths": paths, "morePaths": more}),
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
                    "text": head(&text, MAX_MESSAGE_BYTES),
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
                json!({"text": head(&text, MAX_USER_BYTES), "textBytes": text.len()}),
            ))
        }
        _ => None,
    }
}

/// Best-effort removal of well-known secret shapes from every string the journal keeps, since
/// receipts outlive the turn and can be published to project memory.
fn redacted(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(codex_secrets::redact_secrets(text)),
        Value::Array(items) => Value::Array(items.into_iter().map(redacted).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key, redacted(value)))
                .collect(),
        ),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_)) => other,
    }
}

fn plan_payload(arguments: &Value) -> Value {
    let steps = arguments["plan"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_PLAN_STEPS)
        .map(|step| {
            json!({
                "step": head(step["step"].as_str().unwrap_or_default(), MAX_PLAN_STEP_BYTES),
                "status": head(step["status"].as_str().unwrap_or_default(), 32),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "explanation": arguments["explanation"].as_str().map(|text| head(text, MAX_MESSAGE_BYTES)),
        "steps": steps,
    })
}

/// Publishes the thread's journal suffix not yet published, then delivers every staged
/// publication of the project that is still pending (for example after a crash).
pub(crate) async fn publish_window(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    thread_id: &str,
) {
    let Ok(store) = services.runtime().await else {
        return;
    };
    if let Err(error) = stage(store, project_id, thread_id).await {
        tracing::warn!(%thread_id, %error, "failed to stage a window publication");
    }
    deliver_pending(services, store, project_id).await;
}

/// At the start of a new thread, publishes what earlier sessions of the project left open.
pub(crate) async fn publish_open_windows(services: &ProjectIntelligenceServices, project_id: &str) {
    let Ok(store) = services.runtime().await else {
        return;
    };
    match store.threads_with_unpublished_events(project_id, 20).await {
        Ok(threads) => {
            for thread_id in threads {
                if let Err(error) = stage(store, project_id, &thread_id).await {
                    tracing::warn!(%thread_id, %error, "failed to stage a window publication");
                }
            }
        }
        Err(error) => tracing::warn!(%project_id, %error, "failed to list unpublished windows"),
    }
    deliver_pending(services, store, project_id).await;
}

async fn stage(
    store: &StatefulRunStore,
    project_id: &str,
    thread_id: &str,
) -> Result<(), codex_stateful_runtime::StatefulRunStoreError> {
    let from_seq = store.window_publication_watermark(thread_id).await?;
    let through_seq = store.window_event_watermark(thread_id).await?;
    if through_seq <= from_seq {
        return Ok(());
    }
    // The newest page is what a later session can use; older observations of a very long
    // window stay in the journal and the thread's history.
    let events = store
        .window_events_newest_first(thread_id, from_seq, through_seq, PUBLICATION_PAGE)
        .await?;
    let content = publication_content(thread_id, from_seq, through_seq, &events);
    let entry_id = stable_entry_id(project_id, thread_id, from_seq, through_seq);
    store
        .stage_window_publication(&WindowPublication {
            thread_id: thread_id.to_string(),
            from_seq,
            through_seq,
            project_id: project_id.to_string(),
            entry_id,
            content,
            state: WindowPublicationState::Pending,
        })
        .await?;
    Ok(())
}

async fn deliver_pending(
    services: &ProjectIntelligenceServices,
    store: &StatefulRunStore,
    project_id: &str,
) {
    let pending = match store.pending_window_publications(project_id, 20).await {
        Ok(pending) => pending,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to read pending window publications");
            return;
        }
    };
    if pending.is_empty() {
        return;
    }
    let (Ok(blackboard), Ok(node_id)) = (
        services.blackboard().await,
        services.project_node_id(project_id).await,
    ) else {
        return;
    };
    for publication in pending {
        let Ok(id) = BlackboardEntryId::parse(publication.entry_id.clone()) else {
            continue;
        };
        let value = NewBlackboardEntry {
            project_id: project_id.to_string(),
            node_id: node_id.clone(),
            kind: BlackboardKind::Note,
            content: publication.content.clone(),
            structured_value: None,
            confidence: ConfidenceScore::from_basis_points(10_000)
                .unwrap_or_else(|error| unreachable!("static confidence: {error}")),
            verification: BlackboardVerification::Unverified,
            importance: BlackboardImportance::Low,
            root_promotion: RootPromotion::NotPromoted,
            evidence: Vec::new(),
            premises: Vec::new(),
            provenance: BlackboardProvenance {
                kind: BlackboardProvenanceKind::Maintenance,
                source_id: format!(
                    "window-journal:{}:{}-{}",
                    publication.thread_id, publication.from_seq, publication.through_seq
                ),
            },
        };
        match blackboard.create_entry(id, value).await {
            Ok(_) => {
                if let Err(error) = store
                    .mark_window_publication_published(&publication.thread_id, publication.from_seq)
                    .await
                {
                    tracing::warn!(%error, "failed to acknowledge a window publication");
                }
            }
            Err(error) => tracing::warn!(%error, "failed to publish window receipts"),
        }
    }
}

fn stable_entry_id(project_id: &str, thread_id: &str, from_seq: u64, through_seq: u64) -> String {
    let mut hasher = Sha256::new();
    for part in [
        project_id,
        thread_id,
        &from_seq.to_string(),
        &through_seq.to_string(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("stateful-window-receipts-{:x}", hasher.finalize())
}

/// Host receipts for one journal suffix: what changed and what ran, stated as observations.
fn publication_content(
    thread_id: &str,
    from_seq: u64,
    through_seq: u64,
    newest_first: &[WindowEvent],
) -> String {
    let mut lines = vec![format!(
        "Host-observed work receipts (activity, not verified conclusions) from thread {thread_id}, events {}-{through_seq}.",
        from_seq + 1
    )];
    if through_seq - from_seq > newest_first.len() as u64 {
        lines.push(format!(
            "Only the newest {} observations are summarized; the thread's history holds the rest.",
            newest_first.len()
        ));
    }
    let mut files = Vec::<String>::new();
    for event in newest_first
        .iter()
        .filter(|event| event.event.kind == WindowEventKind::Edit)
    {
        let status = event.event.payload["status"].as_str().unwrap_or("unknown");
        for path in event.event.payload["paths"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let path = path["path"].as_str().unwrap_or_default();
            if !files
                .iter()
                .any(|seen| seen.starts_with(&format!("{path} (")))
            {
                files.push(format!("{path} ({status})"));
            }
        }
    }
    if !files.is_empty() {
        let shown = files.len().min(12);
        lines.push(format!(
            "Files changed through apply_patch, newest first (scripts may have changed others): {}{}.",
            files[..shown].join(", "),
            if files.len() > shown {
                format!(", and {} more", files.len() - shown)
            } else {
                String::new()
            }
        ));
    }
    let commands = newest_first
        .iter()
        .filter(|event| event.event.kind == WindowEventKind::Command)
        .collect::<Vec<_>>();
    if !commands.is_empty() {
        let failed = commands
            .iter()
            .filter(|event| exit_code(event) != Some(0))
            .count();
        lines.push(format!(
            "Commands: {} observed, {failed} without exit code 0.",
            commands.len()
        ));
        if let Some(test) = commands
            .iter()
            .find(|event| is_validation_command(command_text(event)))
        {
            lines.push(format!(
                "Last test or check command: `{}` {} (the process exit code, not a test count).",
                head(command_text(test), 240),
                exit_text(test)
            ));
        }
        if let Some(failure) = commands.iter().find(|event| exit_code(event) != Some(0)) {
            lines.push(format!(
                "Last command without exit code 0: `{}` {}.",
                head(command_text(failure), 240),
                exit_text(failure)
            ));
        }
    }
    if let Some(plan) = newest_first
        .iter()
        .find(|event| event.event.kind == WindowEventKind::Plan)
    {
        let steps = plan.event.payload["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|step| step["status"] != "completed")
            .take(3)
            .map(|step| {
                format!(
                    "{} ({})",
                    step["step"].as_str().unwrap_or_default(),
                    step["status"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        if !steps.is_empty() {
            lines.push(format!(
                "Agent's last plan (its own words) had open: {}.",
                steps.join("; ")
            ));
        }
    }
    let mut content = lines.join("\n");
    if content.len() > MAX_PUBLICATION_BYTES {
        content = head(&content, MAX_PUBLICATION_BYTES);
    }
    content.trim().to_string()
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
    RUNNERS.iter().any(|runner| command.contains(runner))
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
#[path = "window_journal_tests.rs"]
mod tests;
