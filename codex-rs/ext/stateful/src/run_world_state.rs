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
use sha2::Digest;
use sha2::Sha256;

const WORLD_STATE_ID: &str = "stateful_run";
const START_MARKER: &str = "<stateful_run>";
const END_MARKER: &str = "</stateful_run>";
const MAX_BODY_BYTES: usize = 8 * 1024;
const MAX_ESTIMATED_TOKENS: usize = 3 * 1024;
const MAX_RENDERED_STEERING: usize = 5;
const MAX_RENDERED_STEERING_INPUT_BYTES: usize = 1024;
const TRUNCATION_MARKER: &str =
    "\n[Stateful run state truncated; query exact tool state before relying on omitted detail.]";

pub(super) enum RunWorldStateStatus {
    Available {
        run: Box<StatefulRun>,
        obligation: Option<Box<StatefulObligation>>,
        steering: Vec<StatefulSteering>,
        steering_complete: bool,
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
            } => {
                hash(&mut hasher, run.id.as_str());
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
            } => {
                field(&mut output, "Run ID", run.id.as_str());
                field(&mut output, "Run revision", &run.revision.to_string());
                field(
                    &mut output,
                    "Strategy revision",
                    &run.strategy_revision.to_string(),
                );
                field(&mut output, "Mode", mode_name(run.value.mode));
                field(&mut output, "Status", status_name(run.status));
                field(&mut output, "Goal", &run.value.goal);
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
                line(
                    &mut output,
                    "Persistence efficiency: after evidence review, if a semantic obligation update and a run status/result update are both ready, issue them sequentially in one code-mode call rather than spending separate model turns on already-decided persistence.",
                );
                if let Some(strategy) = run.strategy.as_deref() {
                    field(&mut output, "Current strategy", strategy);
                }
                render_steering(&mut output, steering, *steering_complete);
                if let Some(obligation) = obligation {
                    render_packet(&mut output, &obligation.value.packet);
                } else {
                    line(
                        &mut output,
                        "Current obligation: no semantic update has been recorded yet. Record one after the first meaningful learning or strategy decision.",
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
                "- [id {}; {status}; revision {}] {input}{suffix}",
                instruction.id, instruction.revision
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
    let fingerprint = Value::String(status.fingerprint());
    let body = status.render();
    let project_id = status.project_id().to_string();
    let run_id = status.run_id().map(str::to_string);
    WorldStateSectionContribution::new(WORLD_STATE_ID, fingerprint.clone(), move |previous| {
        match previous {
            PreviousWorldStateSection::Known(previous) if previous == &fingerprint => None,
            PreviousWorldStateSection::Unknown => None,
            PreviousWorldStateSection::Absent | PreviousWorldStateSection::Known(_) => {
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (START_MARKER, END_MARKER),
                    body.clone(),
                ))
            }
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
            line(output, &format!("- {label}: {value}"));
        }
    }
}

fn field(output: &mut String, label: &str, value: &str) {
    line(output, &format!("{label}: {value}"));
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
