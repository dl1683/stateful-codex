//! Stable rule identities for explicit memory controls and historical corrections.

use codex_project_intelligence::BlackboardEntryId;
use sha2::Digest;
use sha2::Sha256;

/// Stable identity for a rule's exact wording within a project. Retired or superseded
/// entries are immutable history. Only a fresh explicit addition may allocate a later
/// generation of the same identity.
pub(crate) fn user_rule_entry_id(
    project_id: &str,
    scope_id: Option<&str>,
    clause: &str,
    generation: u32,
) -> Option<BlackboardEntryId> {
    let normalized = clause.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hasher = Sha256::new();
    hasher.update(project_id.as_bytes());
    hasher.update([0]);
    hasher.update(normalized.as_bytes());
    // The same words in two investigations are two rules; project-wide rules keep the
    // identity they always had.
    if let Some(scope_id) = scope_id {
        hasher.update([0]);
        hasher.update(scope_id.as_bytes());
    }
    let digest = hasher.finalize();
    let id = match generation {
        0 => format!("stateful-user-rule-{digest:x}"),
        generation => format!("stateful-user-rule-{digest:x}-{generation}"),
    };
    BlackboardEntryId::parse(id).ok()
}

