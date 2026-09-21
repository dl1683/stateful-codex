use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_thread_store::StoredProject;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

const WORLD_STATE_ID: &str = "stateful_project";
const START_MARKER: &str = "<stateful_project>";
const END_MARKER: &str = "</stateful_project>";
const MAX_BODY_BYTES: usize = 1024;

pub(super) enum ProjectIntelligenceStatus {
    Available(StoredProject),
    Missing { project_id: String },
    Unavailable { project_id: String },
}

impl ProjectIntelligenceStatus {
    fn project_id(&self) -> &str {
        match self {
            Self::Available(project) => &project.id,
            Self::Missing { project_id } | Self::Unavailable { project_id } => project_id,
        }
    }

    fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"codex-stateful-project-v1\0");
        match self {
            Self::Available(project) => {
                hasher.update(b"available\0");
                hash_component(&mut hasher, &project.id);
                hash_component(&mut hasher, &project.name);
                for root in &project.roots {
                    hash_component(&mut hasher, &root.path);
                }
                hasher.update(project.updated_at_ms.to_be_bytes());
            }
            Self::Missing { project_id } => {
                hasher.update(b"missing\0");
                hash_component(&mut hasher, project_id);
            }
            Self::Unavailable { project_id } => {
                hasher.update(b"unavailable\0");
                hash_component(&mut hasher, project_id);
            }
        }
        format!("{:x}", hasher.finalize())
    }

    fn render(&self) -> String {
        let mut body = String::with_capacity(MAX_BODY_BYTES);
        append_line(
            &mut body,
            "The user explicitly selected this durable project. Treat threads as views over the same project intelligence; do not infer or switch projects.",
        );
        append_field(&mut body, "Project ID", self.project_id());
        match self {
            Self::Available(project) => {
                append_field(&mut body, "Project name", &project.name);
                append_line(&mut body, "Project roots:");
                let mut included = 0;
                for root in &project.roots {
                    let before = body.len();
                    append_field(&mut body, "-", &root.path);
                    if body.len() == before {
                        break;
                    }
                    included += 1;
                }
                let omitted = project.roots.len().saturating_sub(included);
                if omitted > 0 {
                    append_line(
                        &mut body,
                        &format!("... {omitted} additional roots omitted"),
                    );
                }
                append_line(
                    &mut body,
                    "The project-intelligence store is not populated yet. Use source files as ground truth until blackboard and context-map state is available.",
                );
            }
            Self::Missing { .. } => append_line(
                &mut body,
                "The selected project no longer exists in the project catalog. Do not treat prior project memory as current; ask the host to repair the selection.",
            ),
            Self::Unavailable { .. } => append_line(
                &mut body,
                "The project catalog is temporarily unavailable. Do not claim project-memory or evidence readiness until it can be resolved.",
            ),
        }
        body
    }
}

pub(super) fn project_world_state_section(
    status: ProjectIntelligenceStatus,
) -> WorldStateSectionContribution {
    let fingerprint = Value::String(status.fingerprint());
    let body = status.render();
    let project_id = status.project_id().to_string();
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
        move |role, text| is_project_fragment(role, text, &project_id)
    })
    .with_retained_fragment_matcher(move |role, text| is_project_fragment(role, text, &project_id))
}

fn hash_component(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn append_field(output: &mut String, label: &str, value: &str) {
    append_line(output, &format!("{label}: {}", single_line(value)));
}

fn append_line(output: &mut String, line: &str) {
    if output.len() >= MAX_BODY_BYTES {
        return;
    }
    let prefix = usize::from(!output.is_empty());
    let available = MAX_BODY_BYTES.saturating_sub(output.len() + prefix);
    if available == 0 {
        return;
    }
    if prefix == 1 {
        output.push('\n');
    }
    let take = floor_char_boundary(line, available);
    output.push_str(&line[..take]);
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            _ => character,
        })
        .collect()
}

fn floor_char_boundary(value: &str, maximum: usize) -> usize {
    let mut boundary = maximum.min(value.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn is_project_fragment(role: &str, text: &str, project_id: &str) -> bool {
    role == "developer"
        && text.trim_start().starts_with(START_MARKER)
        && text.contains(&format!("Project ID: {project_id}"))
        && text.trim_end().ends_with(END_MARKER)
}

#[cfg(test)]
#[path = "world_state_tests.rs"]
mod tests;
