//! Which investigation-scoped rules apply in a thread, and the user's acts that bind a thread
//! to an investigation or end one. A rule limited to an investigation applies only in threads
//! bound to that investigation while it is open; it never constrains unrelated work, and it
//! ends only when the user releases it.

use std::collections::HashSet;

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::ChangeRecord;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::KnowledgeScope;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::ScopeState;

use crate::quotation::Quotations;
use crate::request_scope::RequestScope;
use crate::rule_capture::user_message_source;
use crate::user_rules::normalize;

/// Longest scope title shown in the packet.
const MAX_SHOWN_TITLE_CHARS: usize = 120;
/// Investigations named in one packet note.
const MAX_SHOWN_SCOPES: usize = 3;

/// The scoped rules of one projection that do not apply in a thread, and why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeView {
    /// Rules of other or ended investigations, by entry ID.
    pub(crate) inapplicable: HashSet<String>,
    /// The open investigation this thread continues, if any.
    pub(crate) bound_title: Option<String>,
    /// Open investigations this thread is not part of, by title.
    pub(crate) other_open_titles: Vec<String>,
}

impl ScopeView {
    /// Computes the view for `projection` in `thread_id`. Rules without a scope (and every
    /// legacy rule) apply everywhere.
    pub(crate) async fn load(
        store: &BlackboardStore,
        projection: &RootBlackboardProjection,
        thread_id: &str,
    ) -> Self {
        let project_id = projection.project_id.as_str();
        let rule_ids = projection
            .data
            .iter()
            .filter(|hit| hit.entry.value.kind == BlackboardKind::Instruction)
            .map(|hit| hit.entry.id.clone())
            .collect::<Vec<BlackboardEntryId>>();
        let (contexts, bound, scopes) = match (
            store.knowledge_contexts(project_id, &rule_ids).await,
            store.thread_scope(project_id, thread_id).await,
            store.scopes(project_id, /*state*/ None).await,
        ) {
            (Ok(contexts), Ok(bound), Ok(scopes)) => (contexts, bound, scopes),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                tracing::warn!(%project_id, %error, "failed to load rule scopes; scoped rules shown everywhere");
                return Self::default();
            }
        };
        let bound_open = bound.filter(|scope| scope.state == ScopeState::Open);
        let inapplicable = contexts
            .into_iter()
            .filter(|(_, context)| {
                context.scope_id.as_ref().is_some_and(|scope_id| {
                    bound_open
                        .as_ref()
                        .is_none_or(|bound| &bound.scope_id != scope_id)
                })
            })
            .map(|(id, _)| id)
            .collect();
        let other_open_titles = scopes
            .iter()
            .filter(|scope| scope.state == ScopeState::Open)
            .filter(|scope| {
                bound_open
                    .as_ref()
                    .is_none_or(|bound| bound.scope_id != scope.scope_id)
            })
            .take(MAX_SHOWN_SCOPES)
            .map(|scope| shown_title(&scope.title))
            .collect();
        Self {
            inapplicable,
            bound_title: bound_open.map(|scope| shown_title(&scope.title)),
            other_open_titles,
        }
    }

    /// Removes the rules that do not apply here from `projection` and returns how many.
    /// Every consumer of root aliases (the packet, its deltas, completion) applies this to
    /// the same projection before aliases are assigned.
    pub(crate) fn retain_applicable(&self, projection: &mut RootBlackboardProjection) -> u64 {
        let before = projection.data.len();
        projection
            .data
            .retain(|hit| !self.inapplicable.contains(hit.entry.id.as_str()));
        u64::try_from(before - projection.data.len()).unwrap_or(u64::MAX)
    }

    /// The packet's note about investigations, if any applies.
    pub(crate) fn note(&self, not_applied: u64) -> Option<String> {
        let quoted = |titles: &[String]| {
            titles
                .iter()
                .map(|title| serde_json::Value::String(title.clone()).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match (&self.bound_title, not_applied) {
            (None, 0) => None,
            (Some(title), 0) => Some(format!(
                "- This thread continues the investigation {}; its rules above apply until the user ends it.",
                serde_json::Value::String(title.clone())
            )),
            (bound, count) => Some(format!(
                "- {count} rules belong to an investigation this thread is not part of{}; they do not apply here. If this work continues one, ask the user before following its rules.{}",
                if self.other_open_titles.is_empty() {
                    String::new()
                } else {
                    format!(" (open: {})", quoted(&self.other_open_titles))
                },
                bound.as_ref().map_or(String::new(), |title| format!(
                    " This thread continues {}.",
                    serde_json::Value::String(title.clone())
                )),
            )),
        }
    }
}

fn shown_title(title: &str) -> String {
    match title.char_indices().nth(MAX_SHOWN_TITLE_CHARS) {
        Some((end, _)) => format!("{}...", &title[..end]),
        None => title.to_string(),
    }
}

/// Words with which the user ends the investigation a thread is bound to.
const RELEASE_PHRASES: &[&str] = &[
    "we have agreed on the root cause",
    "we've agreed on the root cause",
    "we agree on the root cause",
    "i agree with the root cause",
    "the investigation is over",
    "the investigation is done",
    "the investigation is closed",
    "this investigation is over",
    "this investigation is done",
    "end the investigation",
    "end this investigation",
    "close the investigation",
    "close this investigation",
    "the ground rules no longer apply",
    "the investigation rules no longer apply",
];

/// At turn start: the user's own words may end the investigation this thread is bound to;
/// otherwise a thread that refers back to earlier work joins the project's single open
/// investigation. With several open, nothing is bound and the packet asks.
pub(crate) async fn observe_turn_start(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
    turn_id: &str,
    text: &str,
    request: RequestScope,
) {
    let result = async {
        let bound = store.thread_scope(project_id, thread_id).await?;
        if let Some(scope) = bound
            .as_ref()
            .filter(|scope| scope.state == ScopeState::Open)
        {
            // The message that opened an investigation states its end condition; it never
            // ends it.
            let source = user_message_source(thread_id, turn_id);
            if scope.opened_source != source && releases(text) {
                store
                    .end_scope(
                        project_id,
                        &scope.scope_id,
                        &source,
                        &release_change(scope, thread_id, turn_id),
                    )
                    .await?;
            }
            return Ok(());
        }
        if request != RequestScope::Continuity {
            return Ok(());
        }
        let open = store.scopes(project_id, Some(ScopeState::Open)).await?;
        if let [only] = open.as_slice() {
            store
                .bind_thread_scope(project_id, thread_id, &only.scope_id)
                .await?;
        }
        Ok::<(), codex_project_intelligence::BlackboardStoreError>(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%project_id, %error, "failed to update investigation scopes");
    }
}

/// Whether the user's own (unquoted) words release the current investigation.
pub(crate) fn releases(text: &str) -> bool {
    let quotations = Quotations::new(text);
    let lower = text.to_ascii_lowercase();
    RELEASE_PHRASES.iter().any(|phrase| {
        lower.match_indices(phrase).any(|(start, matched)| {
            // "until we have agreed ..." states the condition; it does not meet it.
            let before = lower[..start].trim_end();
            let conditional = [
                "until", "till", "unless", "once", "when", "after", "before", "if",
            ]
            .iter()
            .any(|word| before.ends_with(word));
            // A quoted or relayed release is someone else's words.
            !conditional
                && !quotations.relays(start, start + matched.len())
                && normalize(&text[start..start + matched.len()]) == normalize(phrase)
        })
    })
}

fn release_change(scope: &KnowledgeScope, thread_id: &str, turn_id: &str) -> ChangeRecord {
    ChangeRecord {
        operation: ChangeOperation::ScopeEnded,
        origin: ChangeOrigin::HostCapture,
        category: KnowledgeCategory::Rule,
        action_id: None,
        thread_id: Some(thread_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        group_id: None,
        preview: scope.title.clone(),
    }
}

#[cfg(test)]
#[path = "rule_scope_tests.rs"]
mod tests;
