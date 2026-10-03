//! The project packet of a context window that continues after native compaction in the same
//! thread. The compaction summary and the retained user messages carry the conversation, so
//! this packet keeps only what must stay in force (the user's rules, the project identity and
//! the completion revision) and points to memory for everything else.
//!
//! A continuation window holds exactly one such carrier. Later changes arrive as bounded
//! `<stateful_project_update>` corrections, never as a second carrier.

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use serde_json::Value;
use serde_json::json;

use crate::limits::MAX_MODEL_ITEM_BYTES;
use crate::root_blackboard::RootBlackboardStatus;
use crate::root_blackboard::render_user_entries;
use crate::root_blackboard::short_digest;
use crate::visible_root::VisibleRoot;
use crate::visible_root::VisibleRootRegistry;
use crate::world_state::END_MARKER;
use crate::world_state::PRODUCT_INSTALL_DEFAULT;
use crate::world_state::ProjectIntelligenceStatus;
use crate::world_state::START_MARKER;
use crate::world_state::UPDATE_END_MARKER;
use crate::world_state::UPDATE_START_MARKER;
use crate::world_state::WORLD_STATE_ID;
use crate::world_state::append_line;
use crate::world_state::is_project_fragment;

/// Byte bound of the continuation project body (the in-session spec's 2,400-byte rules and
/// pointer budget, markers included). A new window whose rules do not fit whole opens with the
/// full packet instead.
pub(crate) const MAX_CONTINUATION_PROJECT_BYTES: usize =
    2_400 - START_MARKER.len() - END_MARKER.len();

const REVISION_PREFIX: &str = "Project intelligence revision:";

/// Whether the window is being opened or has already opened as a continuation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Admission {
    /// A new window: it opens as a continuation only if every rule fits whole.
    Opening,
    /// The window already holds a continuation carrier: keep it, and say what cannot be shown.
    Opened,
}

pub(crate) struct ContinuationProject {
    project_id: String,
    revision: Option<u64>,
    body: String,
    /// Rule and background entries shown whole: (entry ID, alias, revision).
    complete_entries: Vec<(String, String, u64)>,
}

/// The continuation body for `status`. While opening, `None` means the window must open with
/// the full packet: project memory is not readable, or the user's rules do not fit whole.
pub(crate) fn continuation_project(
    status: &ProjectIntelligenceStatus,
    admission: Admission,
) -> Option<ContinuationProject> {
    let (project_id, project_name, root) = match status {
        ProjectIntelligenceStatus::Available {
            project,
            root_blackboard,
            ..
        } => (
            project.id.as_str(),
            Some(project.name.as_str()),
            match root_blackboard.as_ref() {
                RootBlackboardStatus::Available(root) => Some(root),
                RootBlackboardStatus::NotConfigured | RootBlackboardStatus::Unavailable => None,
            },
        ),
        ProjectIntelligenceStatus::Missing { project_id }
        | ProjectIntelligenceStatus::Unavailable { project_id } => {
            (project_id.as_str(), None, None)
        }
    };
    if admission == Admission::Opening && root.is_none() {
        return None;
    }
    let mut body = String::new();
    append_line(&mut body, &format!("Project ID: {project_id}"));
    if let Some(name) = project_name {
        append_line(&mut body, &format!("Project name: {name}"));
    }
    append_line(
        &mut body,
        "This window continues the same thread after context compaction: the compaction summary and your retained messages carry the conversation, and project memory shown earlier is not repeated. For earlier decisions, reasons, facts or turns, call memory_read once with the whole question.",
    );
    append_line(&mut body, PRODUCT_INSTALL_DEFAULT);
    let Some(root) = root else {
        append_line(
            &mut body,
            "Project memory could not be read for this step. The user's rules shown earlier in this window still apply; do not claim memory readiness until it can be read.",
        );
        return Some(ContinuationProject {
            project_id: project_id.to_string(),
            revision: None,
            body,
            complete_entries: Vec::new(),
        });
    };
    let revision = root.projection.revision;
    append_line(
        &mut body,
        &format!(
            "{REVISION_PREFIX} {revision}. For durableLearning completion pass it as rootRevision; blackboard_query returns the rootAlias of any entry not shown here."
        ),
    );
    let shown = render_user_entries(&mut body, root);
    let fits = shown.incomplete == 0 && body.len() <= MAX_CONTINUATION_PROJECT_BYTES;
    match admission {
        Admission::Opening if !fits => return None,
        Admission::Opened if shown.incomplete > 0 => append_line(
            &mut body,
            &format!(
                "{} of the user's rules or background entries could not be shown whole here; memory_read with the question \"what are my rules\" returns them exactly.",
                shown.incomplete
            ),
        ),
        Admission::Opening | Admission::Opened => {}
    }
    Some(ContinuationProject {
        project_id: project_id.to_string(),
        revision: Some(revision),
        body,
        complete_entries: shown.complete_entries,
    })
}

/// The project section of a continuation window. The carrier is rendered once; a
/// revision-only change is a one-line receipt, and changed lines (a new or corrected rule, an
/// alias shift, a notice) are sent as one exact correction.
pub(crate) fn continuation_project_section(
    project: ContinuationProject,
    visible_root: (VisibleRootRegistry, String),
) -> WorldStateSectionContribution {
    let ContinuationProject {
        project_id,
        revision,
        body,
        complete_entries,
    } = project;
    let lines = stable_lines(&body);
    let fingerprint = short_digest(
        &lines
            .iter()
            .map(|(_, digest)| digest.as_str())
            .collect::<Vec<_>>()
            .join(","),
    );
    let snapshot = json!({
        "mode": "continuation",
        "fingerprint": fingerprint,
        "lines": lines.iter().map(|(key, digest)| json!([key, digest])).collect::<Vec<_>>(),
        "rootRevision": revision,
    });
    let matcher_project_id = project_id.clone();
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot.clone(), move |previous| {
        let (registry, thread_id) = &visible_root;
        let record_visible = || match revision {
            Some(revision) => {
                let mut visible = VisibleRoot::new(revision);
                for (entry_id, alias, entry_revision) in &complete_entries {
                    visible.insert(entry_id.clone(), alias.clone(), *entry_revision);
                }
                registry.record(thread_id, visible);
            }
            None => registry.clear(thread_id),
        };
        let revision_text =
            revision.map_or_else(|| "unknown".to_string(), |revision| revision.to_string());
        match previous {
            PreviousWorldStateSection::Known(previous) if previous == &snapshot => None,
            PreviousWorldStateSection::Known(previous)
                if previous.get("mode") == snapshot.get("mode")
                    && previous.get("fingerprint") == snapshot.get("fingerprint") =>
            {
                if let Some(revision) = revision {
                    registry.advance_revision(thread_id, revision);
                }
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    format!(
                        "Project ID: {project_id}. Project intelligence revision is now {revision_text}; the rules shown are unchanged. Use rootRevision {revision_text} for completion."
                    ),
                ))
            }
            PreviousWorldStateSection::Known(previous)
                if previous.get("mode") == snapshot.get("mode") =>
            {
                record_visible();
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    correction(&project_id, &revision_text, previous, &body),
                ))
            }
            PreviousWorldStateSection::Absent
            | PreviousWorldStateSection::Unknown
            | PreviousWorldStateSection::Known(_) => {
                record_visible();
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (START_MARKER, END_MARKER),
                    body.clone(),
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

/// (key, digest) of each body line except the revision line, which changes with every write.
/// An entry line is keyed by its alias, so a corrected entry replaces the line it had.
fn stable_lines(body: &str) -> Vec<(String, String)> {
    body.lines()
        .filter(|line| !line.starts_with(REVISION_PREFIX))
        .map(|line| {
            let key = line
                .strip_prefix("- ")
                .and_then(|rest| rest.split_whitespace().next())
                .filter(|alias| {
                    alias.len() > 1
                        && alias.starts_with('E')
                        && alias[1..]
                            .chars()
                            .all(|character| character.is_ascii_digit())
                })
                .map_or_else(|| short_digest(line), str::to_string);
            (key, short_digest(line))
        })
        .collect()
}

/// The exact lines that changed since `previous`, and the aliases no longer shown.
fn correction(project_id: &str, revision: &str, previous: &Value, body: &str) -> String {
    let previous_lines = previous
        .get("lines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pair| Some((pair.get(0)?.as_str()?, pair.get(1)?.as_str()?)))
        .collect::<Vec<_>>();
    let current = stable_lines(body);
    let mut text = format!(
        "Project ID: {project_id}. The user's rules or this packet changed; use rootRevision {revision} for completion. These lines replace the earlier lines with the same alias, or are new:"
    );
    for (line, (_, digest)) in body
        .lines()
        .filter(|line| !line.starts_with(REVISION_PREFIX))
        .zip(&current)
    {
        if !previous_lines
            .iter()
            .any(|(_, previous_digest)| previous_digest == digest)
        {
            text.push('\n');
            text.push_str(line);
        }
    }
    let removed = previous_lines
        .iter()
        .filter(|(key, _)| key.starts_with('E'))
        .filter(|(key, _)| !current.iter().any(|(current_key, _)| current_key == key))
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        text.push_str(&format!(
            "\nNo longer shown or in force: {}.",
            removed.join(", ")
        ));
    }
    let bound = MAX_MODEL_ITEM_BYTES - UPDATE_START_MARKER.len() - UPDATE_END_MARKER.len();
    if text.len() > bound {
        let mut end = bound - 120;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(
            "\n[correction shortened; memory_read with \"what are my rules\" returns them exactly]",
        );
    }
    text
}

#[cfg(test)]
#[path = "continuation_tests.rs"]
mod tests;
