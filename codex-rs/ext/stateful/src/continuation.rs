//! The project packet of a context window that continues after native compaction in the same
//! thread. The compaction summary and the retained user messages carry the conversation, so
//! this packet keeps only what must stay in force (the user's rules, the project identity and
//! the completion revision) and points to memory for everything else.

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use serde_json::Value;
use serde_json::json;

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
/// pointer budget). A window whose rules do not fit keeps the full packet instead, so no rule
/// is ever dropped to meet it.
pub(crate) const MAX_CONTINUATION_PROJECT_BYTES: usize = 2_400;

const REVISION_PREFIX: &str = "Project intelligence revision:";

pub(crate) struct ContinuationProject {
    project_id: String,
    revision: u64,
    body: String,
    /// Rule and background entries shown whole: (entry ID, alias, revision).
    complete_entries: Vec<(String, String, u64)>,
}

/// The continuation body for `status`, or `None` when the window must keep the full packet:
/// project memory is not available, or the user's rules do not fit whole.
pub(crate) fn continuation_project(
    status: &ProjectIntelligenceStatus,
) -> Option<ContinuationProject> {
    let ProjectIntelligenceStatus::Available {
        project,
        root_blackboard,
        ..
    } = status
    else {
        return None;
    };
    let RootBlackboardStatus::Available(root) = root_blackboard.as_ref() else {
        return None;
    };
    let revision = root.projection.revision;
    let mut body = String::new();
    append_line(&mut body, &format!("Project ID: {}", project.id));
    append_line(&mut body, &format!("Project name: {}", project.name));
    append_line(
        &mut body,
        "This window continues the same thread after context compaction: the compaction summary and your retained messages carry the conversation, and project memory shown earlier is not repeated. For earlier decisions, reasons, facts or turns, call memory_read once with the whole question.",
    );
    append_line(&mut body, PRODUCT_INSTALL_DEFAULT);
    append_line(
        &mut body,
        &format!(
            "{REVISION_PREFIX} {revision}. For durableLearning completion pass it as rootRevision; blackboard_query returns the rootAlias of any entry not shown here."
        ),
    );
    let shown = render_user_entries(&mut body, root);
    if shown.incomplete > 0 || body.len() > MAX_CONTINUATION_PROJECT_BYTES {
        return None;
    }
    Some(ContinuationProject {
        project_id: project.id.clone(),
        revision,
        body,
        complete_entries: shown.complete_entries,
    })
}

/// The project section of a continuation window. A changed body (a new or corrected rule)
/// is rendered whole; a revision-only change is a one-line receipt.
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
    // The revision line changes with every write; only the rest decides a re-render.
    let stable = body
        .lines()
        .filter(|line| !line.starts_with(REVISION_PREFIX))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let snapshot = json!({
        "mode": "continuation",
        "fingerprint": short_digest(&stable),
        "rootRevision": revision,
    });
    let matcher_project_id = project_id.clone();
    WorldStateSectionContribution::new(WORLD_STATE_ID, snapshot.clone(), move |previous| {
        let (registry, thread_id) = &visible_root;
        match previous {
            PreviousWorldStateSection::Known(previous) if previous == &snapshot => None,
            PreviousWorldStateSection::Known(previous)
                if previous.get("mode") == snapshot.get("mode")
                    && previous.get("fingerprint") == snapshot.get("fingerprint") =>
            {
                registry.advance_revision(thread_id, revision);
                let previous_revision = previous
                    .get("rootRevision")
                    .and_then(Value::as_u64)
                    .map_or_else(|| "unknown".to_string(), |revision| revision.to_string());
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    (UPDATE_START_MARKER, UPDATE_END_MARKER),
                    format!(
                        "Project ID: {project_id}. Project intelligence revision advanced from {previous_revision} to {revision}; the rules shown are unchanged. Use rootRevision {revision} for completion."
                    ),
                ))
            }
            PreviousWorldStateSection::Absent
            | PreviousWorldStateSection::Unknown
            | PreviousWorldStateSection::Known(_) => {
                let mut visible = VisibleRoot::new(revision);
                for (entry_id, alias, entry_revision) in &complete_entries {
                    visible.insert(entry_id.clone(), alias.clone(), *entry_revision);
                }
                registry.record(thread_id, visible);
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

#[cfg(test)]
#[path = "continuation_tests.rs"]
mod tests;
