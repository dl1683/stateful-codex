use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::SteeringStatus;
use codex_stateful_runtime::WorkflowMode;
use serde_json::Value;

use crate::checkpoint::CHECKPOINT_TOOL_CALLS;
use crate::completion::REUSABLE_LEARNING_RULE;
use crate::limits::MAX_MODEL_ITEM_BYTES;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

const WORLD_STATE_ID: &str = "stateful_run";
const START_MARKER: &str = "<stateful_run>";
const END_MARKER: &str = "</stateful_run>";
const UPDATE_START_MARKER: &str = "<stateful_run_update>";
const UPDATE_END_MARKER: &str = "</stateful_run_update>";
/// Markers plus body stay within the 9,000-byte model item bound (one byte per token
/// is the worst case).
const MAX_BODY_BYTES: usize = MAX_MODEL_ITEM_BYTES - START_MARKER.len() - END_MARKER.len();
/// Independent budgets, rendered obligation first, so neither steering nor descriptive
/// text can crowd the current obligation out of the packet.
const MAX_OBLIGATION_BYTES: usize = 3 * 1024;
const MAX_STEERING_BYTES: usize = 5 * 512;
const OBLIGATION_SHORTENED: &str = "Obligation shortened: later items are omitted here. Call stateful_run_read with section=\"obligation\" and follow nextCursor for the exact current obligation.";
const STEERING_SHORTENED: &str = "This bounded view omitted or shortened unresolved steering. Use steering_query to retrieve the exact remaining detail before choosing or revising strategy.";
const MAX_ESTIMATED_TOKENS: usize = 3 * 1024;
const MAX_RENDERED_STEERING: usize = 5;
const MAX_RENDERED_GOAL_BYTES: usize = 2 * 1024;
const MAX_RENDERED_STEERING_INPUT_BYTES: usize = 1024;
const WRITE_TOOLS_ARE_DIRECT: &str = "Stateful write tools (blackboard_record_batch, blackboard_update_batch, blackboard_relate, obligation_update, stateful_run_update, steering_reconcile) are direct function tools and are not callable inside exec.";
const COLLABORATIVE_COMPLETION: &str = "This Collaborative run stays open across the user's turns and the host records every final answer, so do not complete it at the end of a turn. Complete it only when the user says the overall goal is done or asks to close it; completion then covers everything recorded since the run began. To complete, as the final Stateful mutation: stateful_run_update with expectedRevision, status completed, completionDisposition noReusableLearning and result when nothing reusable was learned; otherwise durableLearning with expectedRevision, status completed, completionIdempotencyKey, finalObligation, result, rootRevision and materialRootFindings.";
const TRUNCATION_MARKER: &str = "\n[Stateful run state truncated; call stateful_run_read (goal or obligation) or steering_query before relying on omitted detail.]";

pub(super) enum RunWorldStateStatus {
    Available {
        run: Box<StatefulRun>,
        obligation: Option<Box<StatefulObligation>>,
        steering: Vec<StatefulSteering>,
        steering_complete: bool,
        /// Checkpoint epoch once enough tool calls passed since the last obligation.
        checkpoint_due: Option<u64>,
    },
    Unavailable {
        project_id: String,
    },
}

impl RunWorldStateStatus {
    fn project_id(&self) -> &str {
        match self {
            Self::Available { run, .. } => &run.value.project_id,
            Self::Unavailable { project_id } => project_id,
        }
    }

    fn run_id(&self) -> Option<&str> {
        match self {
            Self::Available { run, .. } => Some(run.id.as_str()),
            Self::Unavailable { .. } => None,
        }
    }

    fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"codex-stateful-run-v1\0");
        hash(&mut hasher, self.project_id());
        match self {
            Self::Available {
                run,
                obligation,
                steering,
                steering_complete,
                checkpoint_due,
            } => {
                hash(&mut hasher, run.id.as_str());
                // Collaborative runs render no checkpoint text, so an epoch change must not
                // produce an otherwise empty update.
                let rendered_checkpoint = match run.value.mode {
                    WorkflowMode::Collaborative => None,
                    WorkflowMode::Autonomous | WorkflowMode::Socratic => *checkpoint_due,
                };
                hasher.update(rendered_checkpoint.unwrap_or_default().to_be_bytes());
                hasher.update(run.revision.to_be_bytes());
                hasher.update(run.strategy_revision.to_be_bytes());
                if let Some(obligation) = obligation {
                    hash(&mut hasher, &obligation.id);
                    hasher.update(obligation.revision.to_be_bytes());
                }
                for instruction in steering {
                    hash(&mut hasher, instruction.id.as_str());
                    hasher.update(instruction.revision.to_be_bytes());
                }
                hasher.update([u8::from(*steering_complete)]);
            }
            Self::Unavailable { .. } => hasher.update(b"unavailable"),
        }
        format!("{:x}", hasher.finalize())
    }

    fn render(&self) -> String {
        let mut output = String::with_capacity(MAX_BODY_BYTES);
        line(
            &mut output,
            "This is the durable Stateful run selected by the user. Its mode, goal, and binding constraints are explicit; do not infer replacements.",
        );
        field(&mut output, "Project ID", self.project_id());
        match self {
            Self::Unavailable { .. } => line(
                &mut output,
                "Run state is unavailable. Do not claim Stateful progress, completion, or steering reconciliation until it can be read.",
            ),
            Self::Available {
                run,
                obligation,
                steering,
                steering_complete,
                checkpoint_due,
            } => {
                field(&mut output, "Run ID", run.id.as_str());
                field(&mut output, "Run revision", &run.revision.to_string());
                line(
                    &mut output,
                    &format!(
                        "Run-update precondition: pass expectedRevision: {} to stateful_run_update. This is the run revision; never substitute the separate project intelligence revision.",
                        run.revision
                    ),
                );
                field(
                    &mut output,
                    "Strategy revision",
                    &run.strategy_revision.to_string(),
                );
                field(&mut output, "Mode", mode_name(run.value.mode));
                field(&mut output, "Status", status_name(run.status));
                if run.status == StatefulRunStatus::Running
                    && run.value.mode != WorkflowMode::Collaborative
                {
                    // The counter is process-local and advisory: after a restart or a run
                    // switch it cannot know earlier calls, so the wording says exactly that.
                    // An unanswered checkpoint escalates, because a soft nudge alone is
                    // frequently ignored.
                    match checkpoint_due {
                        Some(epoch @ 0..=1) => line(
                            &mut output,
                            &format!(
                                "Semantic checkpoint: advisory due (process-local checkpoint {epoch}); this process observed at least {} successful direct tool calls since the counter was last reset. Calls before a restart are unknown. Update the obligation if semantic state materially changed.",
                                epoch * CHECKPOINT_TOOL_CALLS
                            ),
                        ),
                        Some(epoch) => line(
                            &mut output,
                            &format!(
                                "Semantic checkpoint: overdue (process-local checkpoint {epoch}); this process observed at least {} successful direct tool calls with no obligation update. Call obligation_update now with what was learned, changed, or is still uncertain since the last update; if nothing material changed, record only the next decisive step.",
                                epoch * CHECKPOINT_TOOL_CALLS
                            ),
                        ),
                        None => line(
                            &mut output,
                            "Semantic checkpoint: none due from successful direct tool calls observed by this process. Earlier calls may be unknown after a restart or a run switch; semantic change, not this counter, decides whether obligation_update is warranted.",
                        ),
                    }
                }
                if run.value.mode == WorkflowMode::Autonomous {
                    field(
                        &mut output,
                        "Autonomous continuation budget",
                        &format!(
                            "{} of {} used; up to {} seconds elapsed",
                            run.continuations_used,
                            run.value.budget.max_continuations,
                            run.value.budget.max_elapsed_seconds
                        ),
                    );
                }
                match run.value.mode {
                    WorkflowMode::Autonomous => line(
                        &mut output,
                        "Mode obligation: continue useful authorized work without routine checkpoints; emit semantic updates at meaningful changes and stop only at a genuine terminal condition.",
                    ),
                    WorkflowMode::Collaborative => line(
                        &mut output,
                        "Mode obligation: execute normally, keep progress semantically legible, and reconcile steering without turning routine work into approval gates.",
                    ),
                    WorkflowMode::Socratic if run.status == StatefulRunStatus::Pending => line(
                        &mut output,
                        "Mode obligation: question and synthesize only. Do not invoke execution tools or modify project files until the user explicitly resumes this run.",
                    ),
                    WorkflowMode::Socratic => line(
                        &mut output,
                        "Mode obligation: the user explicitly transitioned this Socratic run to execution; follow the agreed strategy and surface unresolved assumptions.",
                    ),
                }
                match run.value.mode {
                    WorkflowMode::Collaborative => line(
                        &mut output,
                        &format!("{WRITE_TOOLS_ARE_DIRECT} {COLLABORATIVE_COMPLETION}"),
                    ),
                    WorkflowMode::Autonomous | WorkflowMode::Socratic => line(
                        &mut output,
                        &format!(
                            "Semantic progress: while work remains, call obligation_update only when learning, strategy, uncertainty, blockers, or next work materially change; explain meaning, not activity. {REUSABLE_LEARNING_RULE} {WRITE_TOOLS_ARE_DIRECT} Completion: if the run learned nothing reusable, finish once with stateful_run_update passing exactly expectedRevision, status completed, completionDisposition noReusableLearning, and result. Otherwise, after recording the reusable findings and all other warranted durable writes, finish once with durableLearning: expectedRevision, status completed, completionIdempotencyKey, finalObligation, result, rootRevision, and materialRootFindings. Completion must be the final Stateful mutation."
                        ),
                    ),
                }
                append_segment(
                    &mut output,
                    MAX_OBLIGATION_BYTES,
                    OBLIGATION_SHORTENED,
                    |segment| match obligation {
                        Some(obligation) => render_packet(segment, &obligation.value.packet),
                        None => line(
                            segment,
                            "Current obligation: no semantic update has been recorded yet. Record one after the first meaningful learning or strategy decision.",
                        ),
                    },
                );
                append_segment(
                    &mut output,
                    MAX_STEERING_BYTES,
                    STEERING_SHORTENED,
                    |segment| {
                        render_steering(segment, steering, *steering_complete);
                    },
                );
                // Descriptive text comes last so a long goal or strategy can never crowd
                // out the binding obligation and steering above.
                let (goal, goal_shortened) = bounded_text(&run.value.goal, MAX_RENDERED_GOAL_BYTES);
                field(
                    &mut output,
                    "Goal",
                    &if goal_shortened {
                        format!(
                            "{goal} [goal shortened; this is the stored run goal and may not remain in retained thread history. Call stateful_run_read with section=\"goal\" and follow nextCursor before relying on omitted constraints.]"
                        )
                    } else {
                        goal.to_string()
                    },
                );
                if let Some(strategy) = run.strategy.as_deref() {
                    let (strategy, strategy_shortened) =
                        bounded_text(strategy, MAX_RENDERED_GOAL_BYTES);
                    field(
                        &mut output,
                        "Current strategy",
                        &if strategy_shortened {
                            format!(
                                "{strategy} [strategy shortened; call stateful_run_read with section=\"strategy\" and follow nextCursor before relying on omitted detail.]"
                            )
                        } else {
                            strategy.to_string()
                        },
                    );
                }
            }
        }
        output
    }
}

fn render_steering(output: &mut String, steering: &[StatefulSteering], steering_complete: bool) {
    if steering.is_empty() {
        if steering_complete {
            line(
                output,
                "Unresolved user steering: none. This current unresolved set is fully represented; do not call steering_query unless historical reconciliation detail is needed.",
            );
        } else {
            line(
                output,
                "Unresolved user steering: none in this bounded view, but later records may exist. Use steering_query before choosing or revising strategy.",
            );
        }
        return;
    }

    line(output, "Unresolved user steering:");
    let mut shortened = false;
    for instruction in steering.iter().take(MAX_RENDERED_STEERING) {
        let status = match instruction.status {
            SteeringStatus::Submitted => "submitted",
            SteeringStatus::Acknowledged => "acknowledged",
            SteeringStatus::Applied => "applied",
            SteeringStatus::Rejected => "rejected",
        };
        let (input, input_shortened) =
            bounded_text(&instruction.value.input, MAX_RENDERED_STEERING_INPUT_BYTES);
        shortened |= input_shortened;
        let suffix = if input_shortened {
            " [input shortened]"
        } else {
            ""
        };
        line(
            output,
            &format!(
                "- [id {}; {status}; revision {}] {}{suffix}",
                instruction.id,
                instruction.revision,
                single_line(input)
            ),
        );
    }
    let complete = steering_complete && steering.len() <= MAX_RENDERED_STEERING && !shortened;
    if complete {
        line(
            output,
            "This current unresolved steering set is fully represented. Reconcile it directly from these IDs and revisions; do not call steering_query unless historical detail is needed.",
        );
    } else {
        line(
            output,
            "This bounded view omitted or shortened unresolved steering. Use steering_query to retrieve the exact remaining detail before choosing or revising strategy.",
        );
    }
    line(
        output,
        "Acknowledge and visibly apply or reject each instruction; never silently drop it.",
    );
}

pub(super) fn run_world_state_section(
    status: RunWorldStateStatus,
) -> WorldStateSectionContribution {
    let body = status.render();
    let project_id = status.project_id().to_string();
    let run_id = status.run_id().map(str::to_string);
    let snapshot = json!({
        "fingerprint": status.fingerprint(),
        "runId": run_id,
        "fields": block_lines(&body, RunBlockKind::Field).map(line_digest).collect::<Vec<_>>(),
        "fieldKeys": block_lines(&body, RunBlockKind::Field).map(field_key).collect::<Vec<_>>(),
        "steering": line_digest(&block_lines(&body, RunBlockKind::Steering).collect::<Vec<_>>().join("\n")),
        "obligation": line_digest(&block_lines(&body, RunBlockKind::Obligation).collect::<Vec<_>>().join("\n")),
    });
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot.clone(), move |previous| {
        match previous {
            PreviousWorldStateSection::Known(previous)
                if previous.get("fingerprint") == snapshot.get("fingerprint") =>
            {
                None
            }
            PreviousWorldStateSection::Known(previous)
                if let Some(delta) = run_delta(previous, &snapshot, &body) =>
            {
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    delta,
                ))
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
    .with_legacy_matcher({
        let project_id = project_id.clone();
        let run_id = run_id.clone();
        move |role, text| matches_fragment(role, text, &project_id, run_id.as_deref())
    })
    .with_retained_fragment_matcher(move |role, text| {
        matches_fragment(role, text, &project_id, run_id.as_deref())
    })
}

/// Renders only what changed in the same run: changed header fields line by line,
/// and the whole obligation or steering block when any of its lines changed, so a
/// replaced packet never mixes with stale items. Returns `None` when a full render
/// is required (different run, legacy snapshot, or no cheaper description).
fn run_delta(previous: &Value, current: &Value, body: &str) -> Option<String> {
    if previous.get("runId") != current.get("runId") || current.get("runId")?.is_null() {
        return None;
    }
    let previous_fields = previous
        .get("fields")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect::<std::collections::HashSet<_>>();
    let current_fields = block_lines(body, RunBlockKind::Field).collect::<Vec<_>>();
    // A field that disappeared cannot be expressed as a replacement line.
    let current_keys = current_fields
        .iter()
        .map(|field| field_key(field))
        .collect::<std::collections::HashSet<_>>();
    if previous
        .get("fieldKeys")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .any(|key| !current_keys.contains(key))
    {
        return None;
    }
    let project_line = body.lines().nth(1).unwrap_or_default();
    let run_id = current.get("runId")?.as_str()?;
    let mut delta = format!(
        "{project_line}. Run ID: {run_id}. These lines replace the matching fields of the retained <stateful_run> packet."
    );
    for field in current_fields
        .iter()
        .filter(|field| !previous_fields.contains(line_digest(field).as_str()))
    {
        delta.push('\n');
        delta.push_str(field);
    }
    for (kind, key, heading) in [
        (
            RunBlockKind::Steering,
            "steering",
            "Unresolved steering (replaces the previous steering view):",
        ),
        (
            RunBlockKind::Obligation,
            "obligation",
            "Semantic obligation (replaces the previous obligation entirely):",
        ),
    ] {
        if previous.get(key) != current.get(key) {
            delta.push('\n');
            delta.push_str(heading);
            for line in block_lines(body, kind) {
                delta.push('\n');
                delta.push_str(line);
            }
        }
    }
    (delta.len() <= body.len() / 2).then_some(delta)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunBlockKind {
    Field,
    Steering,
    Obligation,
}

fn block_lines(body: &str, kind: RunBlockKind) -> impl Iterator<Item = &str> {
    body.lines().filter(move |line| run_block(line) == kind)
}

fn run_block(line: &str) -> RunBlockKind {
    if line.starts_with("Unresolved user steering")
        || line.starts_with("- [id ")
        || line.starts_with("This current unresolved steering")
        || line.starts_with("This bounded view omitted")
        || line.starts_with("Acknowledge and visibly")
    {
        RunBlockKind::Steering
    } else if line.starts_with("Current semantic obligation")
        || line.starts_with("Current obligation:")
        || line.starts_with("Obligation shortened:")
        || [
            "- Examined:",
            "- Why it matters:",
            "- Learned:",
            "- Implication:",
            "- Strategy:",
            "- Changed:",
            "- Next:",
            "- Uncertainty:",
            "- Blockers:",
            "- Useful user judgment:",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    {
        RunBlockKind::Obligation
    } else {
        RunBlockKind::Field
    }
}

fn field_key(line: &str) -> String {
    line.split_once(':')
        .map_or(line, |(key, _)| key)
        .to_string()
}

fn line_digest(line: &str) -> String {
    let digest = Sha256::digest(line.as_bytes());
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn render_packet(output: &mut String, packet: &ObligationPacket) {
    line(output, "Current semantic obligation:");
    for (label, values) in [
        ("Examined", &packet.examined),
        ("Why it matters", &packet.rationale),
        ("Learned", &packet.learning),
        ("Implication", &packet.implication),
        ("Strategy", &packet.strategy),
        ("Changed", &packet.changed),
        ("Next", &packet.next),
        ("Uncertainty", &packet.uncertainty),
        ("Blockers", &packet.blockers),
        ("Useful user judgment", &packet.requested_judgment),
    ] {
        for value in values {
            line(output, &format!("- {label}: {}", single_line(value)));
        }
    }
}

/// Renders one block into its own byte budget and appends it whole. A block over budget
/// keeps its leading lines and ends with `shortened`, which names the exact read.
fn append_segment(
    output: &mut String,
    maximum: usize,
    shortened: &str,
    render: impl FnOnce(&mut String),
) {
    let mut segment = String::new();
    render(&mut segment);
    if segment.len() > maximum {
        const CUT: &str = " …";
        let mut limit = maximum.saturating_sub(shortened.len() + 1 + CUT.len());
        while !segment.is_char_boundary(limit) {
            limit -= 1;
        }
        // Prefer whole lines, but cut inside a line rather than drop most of the budget
        // to one oversized item.
        match segment[..limit].rfind('\n') {
            Some(keep) if keep >= limit / 2 => segment.truncate(keep),
            _ => {
                segment.truncate(limit);
                segment.push_str(CUT);
            }
        }
        if !segment.is_empty() {
            segment.push('\n');
        }
        segment.push_str(shortened);
    }
    for value in segment.lines() {
        line(output, value);
    }
}

fn field(output: &mut String, label: &str, value: &str) {
    line(output, &format!("{label}: {}", single_line(value)));
}

/// Keeps every rendered item on one line so block classification cannot drift.
fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            _ => character,
        })
        .collect()
}

fn line(output: &mut String, value: &str) {
    if output.ends_with(TRUNCATION_MARKER) {
        return;
    }
    let prefix = usize::from(!output.is_empty());
    let content_limit = MAX_BODY_BYTES.saturating_sub(TRUNCATION_MARKER.len());
    let allowed = content_limit.saturating_sub(output.len() + prefix);
    let value_fits = value.len() <= allowed;
    if value_fits {
        let original_len = output.len();
        if prefix == 1 {
            output.push('\n');
        }
        output.push_str(value);
        let marker_tokens =
            codex_utils_string::approx_tokens_from_byte_count(TRUNCATION_MARKER.len());
        if codex_utils_string::approx_tokens_from_byte_count(output.len())
            <= (MAX_ESTIMATED_TOKENS as u64).saturating_sub(marker_tokens)
        {
            return;
        }
        output.truncate(original_len);
    }
    if allowed == 0 && output.is_empty() {
        output.push_str(TRUNCATION_MARKER.trim_start());
        return;
    }
    if prefix == 1 && output.len() < content_limit {
        output.push('\n');
    }
    let mut boundary = allowed.min(value.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    output.push_str(&value[..boundary]);
    output.push_str(TRUNCATION_MARKER);
}

fn bounded_text(value: &str, maximum: usize) -> (&str, bool) {
    if value.len() <= maximum {
        return (value, false);
    }
    let mut boundary = maximum;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (&value[..boundary], true)
}

fn mode_name(mode: WorkflowMode) -> &'static str {
    match mode {
        WorkflowMode::Autonomous => "Autonomous",
        WorkflowMode::Collaborative => "Collaborative",
        WorkflowMode::Socratic => "Socratic",
    }
}

fn status_name(status: StatefulRunStatus) -> &'static str {
    match status {
        StatefulRunStatus::Pending => "pending",
        StatefulRunStatus::Running => "running",
        StatefulRunStatus::Paused => "paused",
        StatefulRunStatus::Completed => "completed",
        StatefulRunStatus::Cancelled => "cancelled",
        StatefulRunStatus::Blocked => "blocked",
        StatefulRunStatus::Failed => "failed",
    }
}

fn hash(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn matches_fragment(role: &str, text: &str, project_id: &str, run_id: Option<&str>) -> bool {
    role == "developer"
        && text.trim_start().starts_with(START_MARKER)
        && text.contains(&format!("Project ID: {project_id}"))
        && run_id.is_none_or(|run_id| text.contains(&format!("Run ID: {run_id}")))
        && text.trim_end().ends_with(END_MARKER)
}

#[cfg(test)]
#[path = "run_world_state_tests.rs"]
mod tests;
