//! References from a new record to the current entries it replaces.
//!
//! An E alias resolves only through what this thread's packet actually showed (the entry
//! and the revision shown), never against a newer projection; an entry ID must carry the
//! revision the caller saw. A user rule can be replaced only by another user rule.

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::MAX_SUPERSEDED_ENTRIES;
use codex_project_intelligence::SupersededEntry;
use serde::Deserialize;
use serde_json::json;

use crate::visible_root::VisibleRootRegistry;

use super::respond;

/// One entry a record replaces: an alias shown in this thread's packet, or an entry ID with
/// the revision the caller saw.
#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum SupersedeReference {
    Alias {
        alias: String,
    },
    Entry {
        #[serde(rename = "entryId")]
        entry_id: String,
        revision: u64,
    },
}

pub(super) fn supersedes_schema() -> serde_json::Value {
    json!({
        "type": "array",
        "maxItems": MAX_SUPERSEDED_ENTRIES,
        "description": "Current entries this record replaces (same subject and scope): an E alias from this thread's packet, or entryId with the revision you saw.",
        "items": {
            "type": "object",
            "properties": {
                "alias": {"type": "string"},
                "entryId": {"type": "string"},
                "revision": {"type": "integer", "minimum": 1}
            },
            "additionalProperties": false
        }
    })
}

/// Resolves references to the exact revisions to replace, refusing anything that is not
/// current, not shown, or a user rule replaced by a non-rule.
pub(super) async fn resolve_superseded(
    store: &BlackboardStore,
    visible_root: &VisibleRootRegistry,
    project_id: &str,
    thread_id: &str,
    successor_is_user_rule: bool,
    references: Vec<SupersedeReference>,
) -> Result<Vec<SupersededEntry>, FunctionCallError> {
    if references.len() > MAX_SUPERSEDED_ENTRIES {
        return Err(respond(format!(
            "supersedes names at most {MAX_SUPERSEDED_ENTRIES} entries"
        )));
    }
    let shown = visible_root.get(thread_id);
    let mut resolved = Vec::with_capacity(references.len());
    for reference in references {
        let (entry_id, revision) = match reference {
            SupersedeReference::Alias { alias } => {
                let (entry_id, revision) = shown
                    .as_ref()
                    .and_then(|root| root.entry_for_alias(&alias))
                    .map(|(entry_id, revision)| (entry_id.to_string(), revision))
                    .ok_or_else(|| {
                        respond(format!(
                            "{alias} is not shown in full in this thread's current packet; pass entryId and revision from blackboard_query"
                        ))
                    })?;
                (entry_id, revision)
            }
            SupersedeReference::Entry { entry_id, revision } => (entry_id, revision),
        };
        let id = BlackboardEntryId::parse(entry_id).map_err(respond)?;
        let current = store
            .get_entry(project_id, &id)
            .await
            .map_err(respond)?
            .ok_or_else(|| respond(format!("entry to replace was not found: {id}")))?;
        if current.state != BlackboardEntryState::Active || current.revision != revision {
            return Err(respond(format!(
                "entry {id} changed since revision {revision}; read it again before replacing it"
            )));
        }
        if current.value.kind == BlackboardKind::Instruction
            && current.value.provenance.kind == BlackboardProvenanceKind::User
            && !successor_is_user_rule
        {
            return Err(respond(
                "a user rule can be replaced only by the user's new rule in their own words",
            ));
        }
        resolved.push(SupersededEntry {
            id,
            expected_revision: revision,
        });
    }
    Ok(resolved)
}
