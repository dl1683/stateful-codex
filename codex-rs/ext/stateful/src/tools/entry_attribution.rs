//! Who stated a memory entry of the user's and when, for model memory reads.

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use serde_json::Value;
use serde_json::json;

use crate::root_blackboard::user_statement;
use crate::root_blackboard::utc_minute;

/// `{"by", "said", "adopted", "recorded"}` for an entry the user declared or applied (the same
/// statement the root renders), `{"by": "user", "recorded"}` for another user-authored revision,
/// and nothing for other entries, whose `provenance` already names their author. Times are UTC
/// minutes; a statement time the host did not observe is `unknown`, never guessed. An
/// unreadable legacy context counts as no statement.
pub(super) async fn attribution(
    store: &BlackboardStore,
    project_id: &str,
    entry: &BlackboardEntry,
) -> Option<Value> {
    let context = store
        .knowledge_context(project_id, &entry.id)
        .await
        .ok()
        .flatten();
    let recorded = utc_minute(entry.created_at_ms);
    match user_statement(context.as_ref()) {
        Some(statement) => Some(json!({
            "by": statement.by, "said": statement.said,
            "adopted": statement.adopted, "recorded": recorded,
        })),
        None => (entry.value.provenance.kind == BlackboardProvenanceKind::User)
            .then(|| json!({"by": "user", "recorded": recorded})),
    }
}
