//! Pure offline entry point for measuring the reducer and renderer on recorded rollouts: no
//! live session, store, executor or model. Not registered; production wiring is stage (c).

use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::PatchApplyStatus;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;

use super::Exclusions;
use super::Observation;
use super::Projection;
use super::SourceRef;
use super::render::CapsuleHeader;
use super::render::render_capsule;
use super::working_files::WitnessOutcome;
use super::working_files::compare_samples;
use super::working_files::sample_file;

/// Recorded thread identity and the boundary the capsule would be built for.
pub struct OfflineCapsuleInput<'a> {
    pub project_id: &'a str,
    pub thread_id: &'a str,
    pub window_id: &'a str,
    /// File contents observed at a turn end (`before`, `None` when never sampled) and at the
    /// boundary (`after`), applied after `after_line` rollout lines have been reduced.
    pub files: &'a [OfflineFileWitness<'a>],
    /// The continuing model's tokenizer; `None` counts one token per byte.
    pub count_tokens: Option<&'a dyn Fn(&str) -> usize>,
}

pub struct OfflineFileWitness<'a> {
    pub relative_path: &'a str,
    pub before: Option<&'a str>,
    pub after: &'a str,
    pub after_line: usize,
}

#[derive(Debug)]
pub struct OfflineCapsule {
    /// The projection payload stage (c) would persist.
    pub projection_json: String,
    /// The rendered capsule, or `None` when even the envelope cannot fit.
    pub capsule: Option<String>,
    pub tokens: usize,
    pub omitted: usize,
    /// Event lines reduced, and lines that were not JSON rollout lines.
    pub events: usize,
    pub unreadable_lines: usize,
}

/// Reduces a recorded rollout (JSONL, `{"type":"event_msg","payload":...}` lines; other
/// lines are skipped) up to a compaction or resume boundary, and renders its capsule.
pub fn offline_capsule(rollout_jsonl: &str, input: &OfflineCapsuleInput<'_>) -> OfflineCapsule {
    let mut projection = Projection::new();
    let (mut turn, mut events, mut unreadable) = (String::from("unknown"), 0, 0);
    let witness = |projection: &mut Projection, file: &OfflineFileWitness<'_>, turn: &str| {
        let mut budget = super::working_files::MAX_TOTAL_SAMPLE_BYTES;
        let path = file.relative_path;
        let mut sample = |text: &str, event: &str| {
            sample_file("offline", ".", path, event, Ok(text.into()), &mut budget)
        };
        let before = file.before.map(|text| sample(text, "before"));
        let after = sample(file.after, "after");
        if let WitnessOutcome::Changed(witness) = compare_samples(before.as_ref(), &after) {
            let source = source(input, turn, &format!("sample:{path}"), None, file.after);
            projection.observe(Observation::FileWitness { source, witness });
        }
    };
    for (index, line) in rollout_jsonl.lines().enumerate() {
        for file in input.files.iter().filter(|file| file.after_line == index) {
            witness(&mut projection, file, &turn);
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            unreadable += usize::from(!line.trim().is_empty());
            continue;
        };
        if value.get("type").and_then(|kind| kind.as_str()) != Some("event_msg") {
            continue;
        }
        let Some(Ok(event)) = value.get("payload").map(EventMsg::deserialize) else {
            continue;
        };
        events += 1;
        let item = format!("line:{index}");
        let observation = match event {
            EventMsg::TurnStarted(started) => {
                turn = started.turn_id;
                continue;
            }
            EventMsg::UserMessage(user) => Observation::UserMessage {
                source: source(input, &turn, &item, None, &user.message),
                text: user.message,
            },
            EventMsg::AgentMessage(agent) => Observation::AssistantMessage {
                source: source(input, &turn, &item, None, &agent.message),
                text: agent.message,
            },
            EventMsg::ExecCommandEnd(end) => Observation::Command {
                source: source(
                    input,
                    &end.turn_id,
                    &item,
                    Some(&end.call_id),
                    &end.aggregated_output,
                ),
                argv: end.command,
                cwd: end.cwd.to_string(),
                exit_code: Some(end.exit_code),
                output: end.aggregated_output,
            },
            EventMsg::PatchApplyEnd(patch) if patch.status == PatchApplyStatus::Completed => {
                touch(&mut projection, patch.changes.keys());
                continue;
            }
            // Current rollouts record typed completed items instead of the legacy events.
            EventMsg::ItemCompleted(done) => match done.item {
                TurnItem::UserMessage(user) => {
                    let text = user.message();
                    let source = source(input, &done.turn_id, &user.id, None, &text);
                    Observation::UserMessage { source, text }
                }
                TurnItem::AgentMessage(agent) => {
                    let text: String = agent
                        .content
                        .iter()
                        .map(|AgentMessageContent::Text { text }| text.as_str())
                        .collect();
                    let source = source(input, &done.turn_id, &agent.id, None, &text);
                    Observation::AssistantMessage { source, text }
                }
                TurnItem::CommandExecution(run) => {
                    let output = run.aggregated_output.unwrap_or_default();
                    Observation::Command {
                        source: source(input, &done.turn_id, &run.id, Some(&run.id), &output),
                        argv: run.command,
                        cwd: run.cwd.to_string(),
                        exit_code: run.exit_code,
                        output,
                    }
                }
                TurnItem::FileChange(change)
                    if change.status == Some(PatchApplyStatus::Completed) =>
                {
                    touch(&mut projection, change.changes.keys());
                    continue;
                }
                _ => continue,
            },
            _ => continue,
        };
        projection.observe(observation);
    }
    let lines = rollout_jsonl.lines().count();
    for file in input.files.iter().filter(|file| file.after_line >= lines) {
        witness(&mut projection, file, &turn);
    }
    let header = CapsuleHeader {
        project: input.project_id,
        thread: input.thread_id,
        window: input.window_id,
        eligibility_generation: 0,
        watermark: events as u64,
    };
    let rendered = render_capsule(
        &projection,
        &header,
        &Exclusions::default(),
        input.count_tokens,
    )
    .ok();
    OfflineCapsule {
        projection_json: projection.to_payload(),
        tokens: rendered.as_ref().map_or(0, |capsule| capsule.tokens),
        omitted: rendered.as_ref().map_or(0, |capsule| capsule.omitted),
        capsule: rendered.map(|capsule| capsule.text),
        events,
        unreadable_lines: unreadable,
    }
}

fn touch<'a>(projection: &mut Projection, paths: impl Iterator<Item = &'a std::path::PathBuf>) {
    let mut paths: Vec<String> = paths.map(|path| path.display().to_string()).collect();
    paths.sort();
    for path in paths {
        projection.observe(Observation::AppliedFileChange { path });
    }
}

/// Offline provenance: the rollout line stands in for the committed item identity.
fn source(
    input: &OfflineCapsuleInput<'_>,
    turn: &str,
    item: &str,
    invocation: Option<&str>,
    enclosure: &str,
) -> SourceRef {
    SourceRef {
        thread_id: input.thread_id.to_string(),
        turn_id: if turn.is_empty() { "unknown" } else { turn }.to_string(),
        item_id: item.to_string(),
        invocation: Some(format!("{turn}:{}", invocation.unwrap_or(item))),
        digest: format!("{:x}", Sha256::digest(enclosure.as_bytes())),
        binding_generation: 1,
        ..SourceRef::default()
    }
}
