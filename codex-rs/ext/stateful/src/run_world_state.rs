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

pub(super) enum RunWorldStateStatus {
    Available {
        run: StatefulRun,
        obligation: Option<Box<StatefulObligation>>,
        steering: Vec<StatefulSteering>,
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
            } => {
                field(&mut output, "Run ID", run.id.as_str());
                field(&mut output, "Mode", mode_name(run.value.mode));
                field(&mut output, "Status", status_name(run.status));
                field(&mut output, "Goal", &run.value.goal);
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
                if let Some(strategy) = run.strategy.as_deref() {
                    field(&mut output, "Current strategy", strategy);
                }
                if let Some(obligation) = obligation {
                    render_packet(&mut output, &obligation.value.packet);
                } else {
                    line(
                        &mut output,
                        "Current obligation: no semantic update has been recorded yet. Record one after the first meaningful learning or strategy decision.",
                    );
                }
                if !steering.is_empty() {
                    line(&mut output, "Unresolved user steering:");
                    for instruction in steering {
                        let status = match instruction.status {
                            SteeringStatus::Submitted => "submitted",
                            SteeringStatus::Acknowledged => "acknowledged",
                            SteeringStatus::Applied => "applied",
                            SteeringStatus::Rejected => "rejected",
                        };
                        line(
                            &mut output,
                            &format!(
                                "- [{}; revision {}] {}",
                                status, instruction.revision, instruction.value.input
                            ),
                        );
                    }
                    line(
                        &mut output,
                        "Acknowledge and visibly apply or reject each instruction; never silently drop it.",
                    );
                }
            }
        }
        output
    }
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
    if output.len() >= MAX_BODY_BYTES {
        return;
    }
    let prefix = usize::from(!output.is_empty());
    let allowed = MAX_BODY_BYTES.saturating_sub(output.len() + prefix);
    if allowed == 0 {
        return;
    }
    if prefix == 1 {
        output.push('\n');
    }
    let mut boundary = allowed.min(value.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    output.push_str(&value[..boundary]);
    if codex_utils_string::approx_tokens_from_byte_count(output.len()) > MAX_ESTIMATED_TOKENS as u64
    {
        output.truncate(output.len().saturating_sub(boundary));
    }
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
