use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_project_intelligence::ProjectRefreshStatus;
use codex_thread_store::StoredProject;
use std::collections::HashMap;
use std::collections::HashSet;

use serde_json::Map;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

use crate::limits::MAX_MODEL_ITEM_BYTES;
use crate::root_blackboard::LaidOutLine;
use crate::root_blackboard::RootBlackboardStatus;
use crate::root_blackboard::RootLayout;
use crate::root_blackboard::render_root_blackboard;
use crate::root_blackboard::short_digest;
use crate::visible_root::VisibleRoot;
use crate::visible_root::VisibleRootRegistry;

const WORLD_STATE_ID: &str = "stateful_project";
const START_MARKER: &str = "<stateful_project>";
const END_MARKER: &str = "</stateful_project>";
const UPDATE_START_MARKER: &str = "<stateful_project_update>";
const UPDATE_END_MARKER: &str = "</stateful_project_update>";
pub(super) const MAX_BODY_BYTES: usize =
    MAX_MODEL_ITEM_BYTES - START_MARKER.len() - END_MARKER.len();
pub(super) const MAX_ESTIMATED_TOKENS: usize = 8 * 1024;
const MAX_PROJECT_ROOT_BYTES: usize = 4 * 1024;
const MAX_DELTA_BYTES: usize = 8 * 1024;
const OMISSION_MARKER: &str =
    "... additional project World State lines omitted by the context bound.";

pub(super) enum ProjectIntelligenceStatus {
    Available {
        project: Box<StoredProject>,
        last_refresh: Option<ProjectRefreshStatus>,
        root_blackboard: Box<RootBlackboardStatus>,
    },
    Missing {
        project_id: String,
    },
    Unavailable {
        project_id: String,
    },
}

impl ProjectIntelligenceStatus {
    fn project_id(&self) -> &str {
        match self {
            Self::Available { project, .. } => &project.id,
            Self::Missing { project_id } | Self::Unavailable { project_id } => project_id,
        }
    }

    fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"codex-stateful-project-v2\0");
        match self {
            Self::Available {
                project,
                last_refresh,
                root_blackboard,
            } => {
                hasher.update(b"available\0");
                hash_component(&mut hasher, &project.id);
                hash_component(&mut hasher, &project.name);
                for root in &project.roots {
                    hash_component(&mut hasher, &root.path);
                }
                hasher.update(project.updated_at_ms.to_be_bytes());
                hash_refresh_status(&mut hasher, last_refresh.as_ref());
                root_blackboard.update_fingerprint(&mut hasher);
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

    fn snapshot(&self, body: &str, layout: &RootLayout) -> Value {
        let mut snapshot = Map::new();
        snapshot.insert("fingerprint".to_string(), Value::String(self.fingerprint()));
        snapshot.insert(
            "semanticFingerprint".to_string(),
            Value::String(semantic_fingerprint(body)),
        );
        snapshot.insert("rootEntries".to_string(), laid_out_keys(&layout.entries));
        snapshot.insert("rootSources".to_string(), laid_out_keys(&layout.sources));
        snapshot.insert(
            "otherLines".to_string(),
            Value::Array(
                other_lines(body, layout)
                    .filter(|line| !is_status_line(line))
                    .map(|line| Value::String(short_digest(line)))
                    .collect(),
            ),
        );
        snapshot.insert(
            "statusLines".to_string(),
            Value::String(short_digest(
                &other_lines(body, layout)
                    .filter(|line| is_status_line(line))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )),
        );
        if let Self::Available {
            root_blackboard, ..
        } = self
            && let RootBlackboardStatus::Available(root) = root_blackboard.as_ref()
        {
            snapshot.insert(
                "rootRevision".to_string(),
                Value::from(root.projection.revision),
            );
        }
        Value::Object(snapshot)
    }

    fn render(&self) -> (String, RootLayout) {
        let mut body = String::with_capacity(MAX_BODY_BYTES);
        let mut layout = RootLayout::default();
        append_line(
            &mut body,
            "The user explicitly selected this durable project. Treat threads as views over the same project intelligence; do not infer or switch projects.",
        );
        append_line(
            &mut body,
            "Start from accumulated project intelligence. For entries labelled verification=sourceVerified and evidence=current, the host rechecked that the cited source bytes still match their stored fingerprints this turn. It did not prove that those bytes entail the entry, that the entry's scope matches this question, that the source is authoritative, or that no other source supersedes it. Reuse these entries without routine rereading; this distinction is not a reason to reverify every claim. Reopen only the smallest decisive range when one of those unchecked dimensions is material, required detail is absent, exact wording/format/code is needed, active findings conflict or remain materially uncertain, evidence is stale/unavailable/uncheckedThisTurn, or the user requests fresh verification. When a few findings plausibly control the answer, compare only those candidates against the requested scope and evidence endpoint. Query focused blackboard knowledge or context-map routes for a missing controlling boundary before reading raw source. After reading missing or changed evidence, update durable state. Bound investigation to the requested outcome: do not turn one criterion or decision dimension into an overall project determination, search by every known filename, or reread the corpus merely to repeat adequate root knowledge. User-confirmed entries are user-supplied premises, not source verification.",
        );
        append_line(
            &mut body,
            "Persist materially reusable understanding: important instructions, facts, numbers, decisions, strategies, questions, contradictions, failures, rejected approaches, signals, and cross-source relationships. Preserve decision-changing contrasts, exact values, qualifiers, scope and authority boundaries, and supersession signals instead of compressing state to only what supports the immediate answer. Require expected reuse value before writing: do not persist cheap-to-recompute inventories, duplicate adequate root knowledge, routine activity, transient progress, or guesses presented as facts. Link the smallest decisive evidence ranges and preserve uncertainty. After one evidence-review pass, commit coherent findings with the bounded batch tool instead of forcing one model round trip per record.",
        );
        append_field(&mut body, "Project ID", self.project_id());
        match self {
            Self::Available {
                project,
                last_refresh,
                root_blackboard,
            } => {
                append_field(&mut body, "Project name", &project.name);
                append_line(&mut body, "Project roots:");
                let roots_start = body.len();
                let mut included = 0;
                for root in &project.roots {
                    let line = format!("- {}", single_line(&root.path));
                    if body
                        .len()
                        .saturating_add(line.len())
                        .saturating_sub(roots_start)
                        > MAX_PROJECT_ROOT_BYTES
                        || !try_append_line(&mut body, &line, /*reserved_bytes*/ 0)
                    {
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
                render_refresh_status(&mut body, last_refresh.as_ref());
                layout = render_root_blackboard(&mut body, root_blackboard);
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
        (body, layout)
    }
}

fn hash_refresh_status(hasher: &mut Sha256, refresh: Option<&ProjectRefreshStatus>) {
    let Some(refresh) = refresh else {
        hasher.update(b"no-refresh\0");
        return;
    };
    hasher.update(b"refresh\0");
    hasher.update([u8::from(refresh.inventory_complete)]);
    hasher.update([u8::from(refresh.region_coverage_complete)]);
    for count in [
        refresh.files_indexed,
        refresh.regions_indexed,
        refresh.files_skipped,
        refresh.missing_files,
    ] {
        hasher.update(count.to_be_bytes());
    }
    hasher.update([u8::from(refresh.truncated)]);
}

fn render_refresh_status(output: &mut String, refresh: Option<&ProjectRefreshStatus>) {
    let Some(refresh) = refresh else {
        append_line(
            output,
            "Source-map refresh health: no completed full refresh is recorded. Do not assume the file inventory or searchable region coverage is complete.",
        );
        return;
    };
    append_line(
        output,
        &format!(
            "Source-map refresh health: inventoryComplete={} regionCoverageComplete={} filesIndexed={} regionsIndexed={} filesSkipped={} missingFiles={} truncated={}.",
            refresh.inventory_complete,
            refresh.region_coverage_complete,
            refresh.files_indexed,
            refresh.regions_indexed,
            refresh.files_skipped,
            refresh.missing_files,
            refresh.truncated,
        ),
    );
    if !refresh.inventory_complete {
        append_line(
            output,
            "Warning: the last full refresh did not complete the file inventory. Do not infer that an unindexed file is absent; refresh before relying on corpus completeness.",
        );
    }
    if !refresh.region_coverage_complete {
        append_line(
            output,
            "Warning: the file inventory completed, but at least one indexed file has only partial searchable region coverage. Use an exact source read when omitted regions could matter.",
        );
    }
}

/// Builds the project section. When `visible_root` is supplied, the render closure
/// records what the model will actually hold: a full render records the fully shown
/// root entries, while a delta clears the record so tools stop compacting.
pub(super) fn project_world_state_section(
    status: ProjectIntelligenceStatus,
    visible_root: Option<(VisibleRootRegistry, String)>,
) -> WorldStateSectionContribution {
    let (body, layout) = status.render();
    let snapshot = status.snapshot(&body, &layout);
    let shown_root = snapshot
        .get("rootRevision")
        .and_then(Value::as_u64)
        .map(|revision| {
            let mut visible = VisibleRoot::new(revision);
            for (entry_id, alias, entry_revision) in &layout.complete_entries {
                visible.insert(entry_id.clone(), alias.clone(), *entry_revision);
            }
            visible
        });
    let project_id = status.project_id().to_string();
    let delta_input = DeltaInput::new(&project_id, &body, &layout);
    let matcher_project_id = project_id.clone();
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot.clone(), move |previous| {
        match previous {
            PreviousWorldStateSection::Known(previous) if previous == &snapshot => None,
            PreviousWorldStateSection::Known(previous)
                if previous.get("semanticFingerprint")
                    == snapshot.get("semanticFingerprint") =>
            {
                if previous.get("rootRevision") == snapshot.get("rootRevision") {
                    return None;
                }
                let previous_revision = previous
                    .get("rootRevision")
                    .and_then(Value::as_u64)
                    .map_or_else(|| "unknown".to_string(), |revision| revision.to_string());
                let current_revision = snapshot
                    .get("rootRevision")
                    .and_then(Value::as_u64)
                    .map_or_else(|| "unknown".to_string(), |revision| revision.to_string());
                let registry_update = snapshot
                    .get("rootRevision")
                    .and_then(Value::as_u64)
                    .map_or(RegistryUpdate::Clear, RegistryUpdate::AdvanceRevision);
                Some(final_fragment(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    format!("Project intelligence revision advanced from {previous_revision} to {current_revision}. The model-visible root blackboard knowledge and source routes are unchanged. Use rootRevision {current_revision} for completion; retain the existing root packet for reasoning and routing."),
                    &project_id,
                    visible_root.as_ref(),
                    registry_update,
                ))
            }
            PreviousWorldStateSection::Known(previous)
                if let Some(delta) = delta_input.render(previous, &snapshot) =>
            {
                Some(final_fragment(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    delta,
                    &project_id,
                    visible_root.as_ref(),
                    RegistryUpdate::Clear,
                ))
            }
            PreviousWorldStateSection::Absent
            | PreviousWorldStateSection::Unknown
            | PreviousWorldStateSection::Known(_) => {
                Some(final_fragment(
                    "developer",
                    (START_MARKER, END_MARKER),
                    body.clone(),
                    &project_id,
                    visible_root.as_ref(),
                    RegistryUpdate::Record(shown_root.clone()),
                ))
            }
        }
    })
    .with_legacy_matcher({
        let project_id = matcher_project_id.clone();
        move |role, text| is_project_fragment(role, text, &project_id)
    })
    .with_retained_fragment_matcher(move |role, text| {
        is_project_fragment(role, text, &matcher_project_id)
    })
}

enum RegistryUpdate {
    Record(Option<VisibleRoot>),
    AdvanceRevision(u64),
    Clear,
}

fn final_fragment(
    role: &'static str,
    markers: (&'static str, &'static str),
    body: impl Into<String>,
    project_id: &str,
    visible_root: Option<&(VisibleRootRegistry, String)>,
    update: RegistryUpdate,
) -> RenderedWorldStateFragment {
    let mut body = body.into();
    let within_bound = |body: &str| {
        markers
            .0
            .len()
            .saturating_add(body.len())
            .saturating_add(markers.1.len())
            <= MAX_MODEL_ITEM_BYTES
    };
    let overflowed = !within_bound(&body);
    if overflowed {
        clear_visible_root(visible_root);
        body = format!(
            "Project ID: {project_id}. Project World State exceeded its hard byte bound; visible root aliases were cleared."
        );
        if !within_bound(&body) {
            body = "Project World State exceeded its hard byte bound; visible root aliases were cleared."
                .to_string();
        }
    }
    assert!(within_bound(&body));
    if !overflowed {
        match update {
            RegistryUpdate::Record(Some(root)) => {
                if let Some((registry, thread_id)) = visible_root {
                    registry.record(thread_id, root);
                }
            }
            RegistryUpdate::Record(None) | RegistryUpdate::Clear => {
                clear_visible_root(visible_root);
            }
            RegistryUpdate::AdvanceRevision(revision) => {
                if let Some((registry, thread_id)) = visible_root {
                    registry.advance_revision(thread_id, revision);
                }
            }
        }
    }
    RenderedWorldStateFragment::new(role, markers, body)
}

fn clear_visible_root(visible_root: Option<&(VisibleRootRegistry, String)>) {
    if let Some((registry, thread_id)) = visible_root {
        registry.clear(thread_id);
    }
}

/// Current packet material needed to describe a root change as a bounded delta
/// against the previously rendered packet instead of replaying the whole root.
struct DeltaInput {
    project_id: String,
    body_len: usize,
    entries: Vec<(String, String, String)>,
    sources: Vec<(String, String, String)>,
    other: Vec<String>,
}

impl DeltaInput {
    fn new(project_id: &str, body: &str, layout: &RootLayout) -> Self {
        let triples = |lines: &[LaidOutLine]| {
            lines
                .iter()
                .map(|line| (line.key.clone(), line.digest.clone(), line.line.clone()))
                .collect()
        };
        Self {
            project_id: project_id.to_string(),
            body_len: body.len(),
            entries: triples(&layout.entries),
            sources: triples(&layout.sources),
            other: other_lines(body, layout).map(str::to_string).collect(),
        }
    }

    /// Describes the change from `previous` in at most half the full packet, or
    /// returns `None` when only a full render is honest or cheaper.
    fn render(&self, previous: &Value, current: &Value) -> Option<String> {
        let previous_entries = parse_keys(previous.get("rootEntries")?)?;
        let previous_sources = parse_keys(previous.get("rootSources")?)?;
        let previous_other = previous
            .get("otherLines")?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .collect::<HashSet<_>>();
        let current_other = self
            .other
            .iter()
            .filter(|line| !is_status_line(line))
            .map(|line| short_digest(line))
            .collect::<HashSet<_>>();
        // A removed or rewritten status line cannot be retracted by appending text.
        if previous_other
            .iter()
            .any(|digest| !current_other.contains(*digest))
        {
            return None;
        }
        let revision = |snapshot: &Value| {
            snapshot
                .get("rootRevision")
                .and_then(Value::as_u64)
                .map_or_else(|| "unknown".to_string(), |revision| revision.to_string())
        };
        let current_revision = revision(current);
        let mut delta = format!(
            "Project ID: {}. Project intelligence revision advanced from {} to {current_revision}. This update amends the retained <stateful_project> packet: read both together, apply alias changes to every earlier reference, and use rootRevision {current_revision} for completion.",
            self.project_id,
            revision(previous),
        );
        describe_changes(&mut delta, "E", &previous_entries, &self.entries);
        describe_changes(&mut delta, "S", &previous_sources, &self.sources);
        let changed_other = self
            .other
            .iter()
            .filter(|line| !is_status_line(line))
            .filter(|line| !previous_other.contains(short_digest(line).as_str()))
            .collect::<Vec<_>>();
        if !changed_other.is_empty() {
            delta.push_str("\nOther changed packet lines:");
            for line in changed_other {
                delta.push('\n');
                delta.push_str(line);
            }
        }
        if previous.get("statusLines") != current.get("statusLines") {
            delta.push_str("\nCurrent status lines (replace every earlier status line):");
            let status = self
                .other
                .iter()
                .filter(|line| is_status_line(line))
                .collect::<Vec<_>>();
            if status.is_empty() {
                delta.push_str("\n(none)");
            }
            for line in status {
                delta.push('\n');
                delta.push_str(line);
            }
        }
        (delta.len() <= (self.body_len / 2).min(MAX_DELTA_BYTES)).then_some(delta)
    }
}

fn describe_changes(
    delta: &mut String,
    prefix: &str,
    previous: &[(String, String)],
    current: &[(String, String, String)],
) {
    let previous_positions = previous
        .iter()
        .enumerate()
        .map(|(index, (key, digest))| (key.as_str(), (index + 1, digest.as_str())))
        .collect::<HashMap<_, _>>();
    let current_keys = current
        .iter()
        .map(|(key, _, _)| key.as_str())
        .collect::<HashSet<_>>();
    let mut renumbered = Vec::new();
    let mut changed = Vec::new();
    for (index, (key, digest, line)) in current.iter().enumerate() {
        match previous_positions.get(key.as_str()) {
            Some((previous_index, previous_digest)) if previous_digest == digest => {
                if *previous_index != index + 1 {
                    renumbered.push(format!("{prefix}{previous_index}->{prefix}{}", index + 1));
                }
            }
            _ => changed.push(line.as_str()),
        }
    }
    let removed = previous
        .iter()
        .enumerate()
        .filter(|(_, (key, _))| !current_keys.contains(key.as_str()))
        .map(|(index, _)| format!("former {prefix}{}", index + 1))
        .collect::<Vec<_>>();
    if !renumbered.is_empty() {
        delta.push_str(&format!(
            "\n{prefix} alias changes (same content, renumbered): {}",
            renumbered.join(", ")
        ));
    }
    if !removed.is_empty() {
        delta.push_str(&format!(
            "\nNo longer shown in the root packet (demoted, superseded, retired, or omitted by the context bound): {}",
            removed.join(", ")
        ));
    }
    if !changed.is_empty() {
        delta.push_str(&format!(
            "\nNew or changed {prefix} lines (current aliases):"
        ));
        for line in changed {
            delta.push('\n');
            delta.push_str(line);
        }
    }
}

/// Status lines are replaced as one block in a delta instead of forcing a full render.
fn is_status_line(line: &str) -> bool {
    line.starts_with("Source-map refresh health:")
        || line.starts_with("Warning:")
        || line.starts_with("- No knowledge has been promoted")
        || line.contains("active candidate entries await")
        || line.contains("root entries omitted by the context bound")
}

fn laid_out_keys(lines: &[LaidOutLine]) -> Value {
    Value::Array(
        lines
            .iter()
            .map(|line| Value::String(format!("{}:{}", line.key, line.digest)))
            .collect(),
    )
}

fn parse_keys(value: &Value) -> Option<Vec<(String, String)>> {
    value
        .as_array()?
        .iter()
        .map(|item| {
            let (key, digest) = item.as_str()?.split_once(':')?;
            Some((key.to_string(), digest.to_string()))
        })
        .collect()
}

fn other_lines<'a>(body: &'a str, layout: &'a RootLayout) -> impl Iterator<Item = &'a str> {
    let laid_out = layout
        .entries
        .iter()
        .chain(&layout.sources)
        .map(|line| line.line.as_str())
        .collect::<HashSet<_>>();
    body.lines().filter(move |line| {
        !laid_out.contains(line) && !line.starts_with("Project intelligence revision: ")
    })
}

fn semantic_fingerprint(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-stateful-project-semantic-v1\0");
    for line in body.lines() {
        if line.starts_with("Project intelligence revision: ") {
            hasher.update(b"Project intelligence revision: <current>\n");
        } else {
            hasher.update(line.as_bytes());
            hasher.update(b"\n");
        }
    }
    format!("{:x}", hasher.finalize())
}

pub(super) fn hash_component(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn append_field(output: &mut String, label: &str, value: &str) {
    append_line(output, &format!("{label}: {}", single_line(value)));
}

pub(super) fn append_line(output: &mut String, line: &str) {
    let prefix = usize::from(!output.is_empty());
    let next_len = output
        .len()
        .saturating_add(prefix)
        .saturating_add(line.len());
    let marker_reserve = OMISSION_MARKER.len() + 1;
    if next_len.saturating_add(marker_reserve) <= MAX_BODY_BYTES {
        if prefix == 1 {
            output.push('\n');
        }
        output.push_str(line);
    } else {
        append_omission_marker(output);
    }
}

pub(super) fn try_append_line(output: &mut String, line: &str, reserved_bytes: usize) -> bool {
    let prefix = usize::from(!output.is_empty());
    let next_len = output
        .len()
        .saturating_add(prefix)
        .saturating_add(line.len());
    if next_len
        .saturating_add(reserved_bytes)
        .saturating_add(OMISSION_MARKER.len() + 1)
        > MAX_BODY_BYTES
        || codex_utils_string::approx_tokens_from_byte_count(next_len) > MAX_ESTIMATED_TOKENS as u64
    {
        return false;
    }
    if prefix == 1 {
        output.push('\n');
    }
    output.push_str(line);
    true
}

fn append_omission_marker(output: &mut String) {
    if output.lines().any(|line| line == OMISSION_MARKER) {
        return;
    }
    let prefix = usize::from(!output.is_empty());
    if output
        .len()
        .saturating_add(prefix)
        .saturating_add(OMISSION_MARKER.len())
        <= MAX_BODY_BYTES
    {
        if prefix == 1 {
            output.push('\n');
        }
        output.push_str(OMISSION_MARKER);
    }
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

fn is_project_fragment(role: &str, text: &str, project_id: &str) -> bool {
    role == "developer"
        && text.trim_start().starts_with(START_MARKER)
        && text.contains(&format!("Project ID: {project_id}"))
        && text.trim_end().ends_with(END_MARKER)
}

#[cfg(test)]
#[path = "world_state_tests.rs"]
mod tests;
