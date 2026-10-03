//! Which investigation-scoped rules apply in a thread. A rule limited to an investigation
//! applies only in threads bound to that investigation while it is open; it never constrains
//! unrelated work. A thread is bound when its message opens the investigation or when the user
//! joins it explicitly (/memory join, statefulMemory/scope); an investigation ends only by the
//! user's explicit end. Natural-language release and joining are not inferred.

use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::ScopeState;
use codex_project_intelligence::ThreadScopes;

/// Root entries a packet or completion considers.
pub(crate) const ROOT_ENTRIES: u32 = 256;
/// Longest scope title shown in the packet.
const MAX_SHOWN_TITLE_CHARS: usize = 120;
/// Investigations named in one packet note.
const MAX_SHOWN_SCOPES: usize = 3;

/// Why some rules do not apply in a thread, from the snapshot of the projection itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeView {
    /// The open investigation this thread continues, if any.
    pub(crate) bound_title: Option<String>,
    /// The investigation this thread continued until the user ended it.
    pub(crate) ended_title: Option<String>,
    /// Open investigations this thread is not part of, by title.
    pub(crate) other_open_titles: Vec<String>,
    /// Rules left out because they belong to an open investigation this thread does not
    /// continue.
    pub(crate) scoped_elsewhere: u64,
    /// Older rules naming a piece of work that no scope was recorded for, left out everywhere.
    pub(crate) unscoped_legacy: u64,
}

/// The project's root projection with only the rules that apply in `thread_id`, and the view
/// that explains what was left out. Storage decides applicability (other investigations,
/// ended ones, older rules naming an unrecorded one) before its bound, in one read
/// transaction with the thread's binding and the investigations, so the projection, its
/// aliases and this view always describe one state. A failure is an error: unknown
/// applicability is never widened.
pub(crate) async fn applicable_projection(
    store: &BlackboardStore,
    project_id: &str,
    thread_id: &str,
) -> Result<(RootBlackboardProjection, ScopeView), BlackboardStoreError> {
    let (projection, scopes) = store
        .root_projection_for_thread(
            RootBlackboardQuery {
                project_id: project_id.to_string(),
                max_entries: ROOT_ENTRIES,
            },
            thread_id,
        )
        .await?;
    Ok((projection, ScopeView::from_snapshot(scopes)))
}

impl ScopeView {
    /// The view of `scopes`, read in the projection's snapshot.
    pub(crate) fn from_snapshot(scopes: ThreadScopes) -> Self {
        let ended_title = scopes
            .bound
            .as_ref()
            .filter(|scope| scope.state == ScopeState::Ended)
            .map(|scope| shown_title(&scope.title));
        let bound_open = scopes.bound.filter(|scope| scope.state == ScopeState::Open);
        let other_open_titles = scopes
            .scopes
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
            bound_title: bound_open.map(|scope| shown_title(&scope.title)),
            ended_title,
            other_open_titles,
            scoped_elsewhere: scopes.scoped_elsewhere,
            unscoped_legacy: scopes.legacy_held_back,
        }
    }

    /// The packet's note about investigations, if any applies.
    pub(crate) fn note(&self) -> Option<String> {
        let quoted = |titles: &[String]| {
            titles
                .iter()
                .map(|title| serde_json::Value::String(title.clone()).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut lines = Vec::new();
        if let Some(title) = &self.bound_title {
            lines.push(format!(
                "- This thread continues the investigation {}; its rules above apply until the user ends it.",
                serde_json::Value::String(title.clone())
            ));
        }
        if let Some(title) = &self.ended_title {
            lines.push(format!(
                "- The investigation {} ended; its rules no longer apply, even where shown earlier.",
                serde_json::Value::String(title.clone())
            ));
        }
        let other = self.scoped_elsewhere;
        if other > 0 {
            lines.push(format!(
                "- {other} rules belong to an investigation this thread is not part of{}; they do not apply here. If this work continues one, ask the user before following its rules.",
                if self.other_open_titles.is_empty() {
                    String::new()
                } else {
                    format!(" (open: {})", quoted(&self.other_open_titles))
                },
            ));
        }
        if self.unscoped_legacy > 0 {
            lines.push(format!(
                "- {} older rules name an investigation but none was recorded for them; they are not applied. Ask the user whether they still apply.",
                self.unscoped_legacy
            ));
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

fn shown_title(title: &str) -> String {
    match title.char_indices().nth(MAX_SHOWN_TITLE_CHARS) {
        Some((end, _)) => format!("{}...", &title[..end]),
        None => title.to_string(),
    }
}

#[cfg(test)]
#[path = "rule_scope_tests.rs"]
mod tests;
