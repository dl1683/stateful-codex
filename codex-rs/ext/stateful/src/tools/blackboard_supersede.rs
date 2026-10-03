//! References from a new record to the current entries it replaces.
//!
//! An E alias resolves only through what this thread's packet actually showed (the entry
//! and the revision shown), never against a newer projection; an entry ID must carry the
//! revision the caller saw. A user rule can be replaced only by another user rule.

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::MAX_SUPERSEDED_ENTRIES;
use codex_project_intelligence::NewBlackboardEntry;
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

/// The stored successor when this exact replacement already committed: the successor is
/// current with the requested value, and it replaced exactly the referenced entries (an
/// entry reference at the revision given; an alias at the revision this thread was shown).
/// `None` means it has not committed; a different committed replacement is refused.
pub(super) async fn committed_succession(
    store: &BlackboardStore,
    visible_root: &VisibleRootRegistry,
    project_id: &str,
    thread_id: &str,
    id: &BlackboardEntryId,
    value: &NewBlackboardEntry,
    references: &[SupersedeReference],
) -> Result<Option<BlackboardEntry>, FunctionCallError> {
    let Some(existing) = store.get_entry(project_id, id).await.map_err(respond)? else {
        return Ok(None);
    };
    let replaced = store.superseded_by(project_id, id).await.map_err(respond)?;
    let same_value = existing.state == BlackboardEntryState::Active
        && NewBlackboardEntry {
            root_promotion: existing.value.root_promotion,
            provenance: existing.value.provenance.clone(),
            ..value.clone()
        } == existing.value;
    // A repeated call named what an earlier call resolved: the packet record current then,
    // which a later delta may have cleared.
    let shown = [
        visible_root.get(thread_id),
        visible_root.last_cleared(thread_id),
    ];
    // The references must resolve, all against one packet record (the current one or the
    // one a later delta cleared), to exactly the committed predecessors: a later full packet
    // can reuse an alias for another entry, so aliases are never mixed across records.
    let snapshots = shown.iter().flatten().map(Some).collect::<Vec<_>>();
    let snapshots = if snapshots.is_empty() {
        vec![None]
    } else {
        snapshots
    };
    let same_replacement = snapshots.into_iter().any(|root| {
        let resolved = references
            .iter()
            .map(|reference| match reference {
                SupersedeReference::Alias { alias } => root
                    .and_then(|root| root.entry_for_alias(alias))
                    .map(|(entry_id, revision)| (entry_id.to_string(), revision)),
                SupersedeReference::Entry { entry_id, revision } => {
                    Some((entry_id.clone(), *revision))
                }
            })
            .collect::<Option<std::collections::HashSet<_>>>();
        resolved.is_some_and(|resolved| {
            resolved.len() == references.len()
                && resolved.len() == replaced.len()
                && replaced.iter().all(|entry| {
                    resolved.iter().any(|(entry_id, revision)| {
                        entry.id.as_str() == entry_id
                            && entry.revision == revision.saturating_add(1)
                    })
                })
        })
    });
    if same_value && same_replacement {
        return Ok(Some(existing));
    }
    Err(respond(format!(
        "{id} was already recorded with a different value or replacement; read it with blackboard_query and record any change under a new idempotencyKey"
    )))
}

/// Resolves references to the exact revisions to replace, refusing anything that is not
/// current, not shown, or a user rule replaced by a non-rule.
/// Why the model cannot close an open check.
pub(super) const OPEN_CHECK_STAYS_OPEN: &str = "this open check stays open until the user closes it with /memory: record what you found as a finding (with evidence_read receipts if it rests on source) and say in your answer that it may settle the check; a passing run or a finished task does not close it";

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

#[cfg(test)]
#[path = "blackboard_supersede_tests.rs"]
mod tests;
