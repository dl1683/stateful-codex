//! Bounded root projection with historical scoped rules held back everywhere.

use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::ThreadScopes;

/// Root entries a packet or completion considers.
pub(crate) const ROOT_ENTRIES: u32 = 256;
/// Historical rules held back from application, without enumerating investigations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScopeView {
    pub(crate) scoped_held_back: u64,
    pub(crate) unscoped_legacy: u64,
}

/// Root projection and quarantine counts from one read snapshot. Scoped and legacy words
/// remain readable history and cannot gain project-wide application.
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
    pub(crate) fn from_snapshot(scopes: ThreadScopes) -> Self {
        Self {
            scoped_held_back: scopes.scoped_held_back,
            unscoped_legacy: scopes.legacy_held_back,
        }
    }

    /// Historical scoped words stay readable, but never become project-wide authority.
    pub(crate) fn note(&self) -> Option<String> {
        let mut lines = Vec::new();
        if self.scoped_held_back > 0 {
            lines.push(format!("- {} historical scoped entries are held back: investigations and scoped application are unsupported. Their words remain readable as history.", self.scoped_held_back));
        }
        if self.unscoped_legacy > 0 {
            lines.push(format!("- {} older rules name an investigation but none was recorded for them; they are not applied. Ask the user whether they still apply.", self.unscoped_legacy));
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}
