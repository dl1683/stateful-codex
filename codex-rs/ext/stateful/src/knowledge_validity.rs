//! What the packet and recall say about entries that are no longer plainly current, and how
//! an invalidation is recorded with an entry's context.
//!
//! Obsolete and historical entries are history only (the root projection already leaves
//! them out); entries that need a check are shown with the reason, so nobody relies on them
//! unchecked. Validity is always read at the revision whose content is shown.

use std::collections::HashMap;

use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::KnowledgeAuthority;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeContext;
use codex_project_intelligence::KnowledgeValidity;
use codex_project_intelligence::RootBlackboardProjection;
use serde_json::Value;
use serde_json::json;

/// Longest invalidation reason shown with an entry.
const MAX_REASON_BYTES: usize = 240;

/// The validity shown with one entry revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownValidity {
    Known(KnowledgeValidity),
    /// The entry's metadata could not be read; it is not presumed current.
    Unavailable,
}

/// The validity and invalidation reason in effect at `entry`'s own revision.
pub(crate) async fn validity_of(
    store: &BlackboardStore,
    entry: &BlackboardEntry,
) -> (ShownValidity, Option<String>) {
    match store
        .knowledge_context_at(&entry.value.project_id, &entry.id, entry.revision)
        .await
    {
        Ok(Some(context)) => (
            ShownValidity::Known(context.validity),
            invalidation_reason(&context),
        ),
        Ok(None) => (ShownValidity::Known(KnowledgeValidity::Current), None),
        Err(error) => {
            tracing::warn!(entry_id = %entry.id, %error, "failed to read entry validity");
            (ShownValidity::Unavailable, None)
        }
    }
}

/// Why an entry was invalidated, as recorded with its context.
fn invalidation_reason(context: &KnowledgeContext) -> Option<String> {
    let payload = serde_json::from_str::<Value>(context.payload.as_deref()?).ok()?;
    let reason = payload["invalidation"]["reason"].as_str()?;
    let reason = reason.replace(['\r', '\n'], " ");
    if reason.len() <= MAX_REASON_BYTES {
        return Some(reason);
    }
    let mut end = MAX_REASON_BYTES;
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}...", &reason[..end]))
}

/// What the root says about its entries' validity: why each entry that needs a check (or
/// whose validity could not be read) may be outdated, by entry ID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RootValidity {
    pub(crate) needs_check: HashMap<String, String>,
}

/// The validity of every projected root entry, read at the projected revision.
pub(crate) async fn root_validity(
    store: &BlackboardStore,
    projection: &RootBlackboardProjection,
) -> RootValidity {
    let mut validity = RootValidity::default();
    for hit in &projection.data {
        match validity_of(store, &hit.entry).await {
            (ShownValidity::Known(KnowledgeValidity::NeedsCheck), reason) => {
                validity.needs_check.insert(
                    hit.entry.id.to_string(),
                    reason.unwrap_or_else(|| "it may no longer be current".to_string()),
                );
            }
            (ShownValidity::Unavailable, _) => {
                validity.needs_check.insert(
                    hit.entry.id.to_string(),
                    "whether it is still current could not be read".to_string(),
                );
            }
            (
                ShownValidity::Known(
                    KnowledgeValidity::Current
                    | KnowledgeValidity::Obsolete
                    | KnowledgeValidity::Historical,
                ),
                _,
            ) => {}
        }
    }
    validity
}

/// The qualification placed right after a needs-check entry's alias, before its content, so
/// truncation of a long line never removes it.
pub(crate) fn needs_check_prefix(reason: &str) -> String {
    format!(" [may be outdated; check before relying on it: {reason}]")
}

/// `context` (or a legacy context when there is none) with `validity` and the invalidation
/// `detail` recorded in its payload. Other payload fields are kept; a payload that is not a
/// JSON object is kept under `previous`.
pub(crate) fn invalidated_context(
    context: Option<KnowledgeContext>,
    validity: KnowledgeValidity,
    detail: Value,
) -> KnowledgeContext {
    let context = context.unwrap_or_else(|| {
        KnowledgeContext::new(KnowledgeCategory::Legacy, KnowledgeAuthority::LegacyUnknown)
    });
    let mut payload = match context.payload.as_deref() {
        None => json!({}),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(object @ Value::Object(_)) => object,
            Ok(_) | Err(_) => json!({ "previous": text }),
        },
    };
    payload["invalidation"] = detail;
    KnowledgeContext {
        validity,
        payload: Some(payload.to_string()),
        ..context
    }
}
