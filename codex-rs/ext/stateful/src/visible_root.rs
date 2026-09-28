use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

/// Records, per thread, which root blackboard entries the model has been shown in
/// full, so tool results can reference those entries by alias instead of repeating
/// prose that is already in model context.
#[derive(Clone, Default)]
pub(crate) struct VisibleRootRegistry {
    threads: Arc<Mutex<HashMap<String, VisibleRoot>>>,
}

impl VisibleRootRegistry {
    pub(crate) fn record(&self, thread_id: &str, root: VisibleRoot) {
        self.threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(thread_id.to_string(), root);
    }

    /// Keeps the shown entries when only the project revision advanced without any
    /// model-visible change.
    pub(crate) fn advance_revision(&self, thread_id: &str, project_revision: u64) {
        if let Some(root) = self
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(thread_id)
        {
            root.project_revision = project_revision;
        }
    }

    pub(crate) fn clear(&self, thread_id: &str) {
        self.threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(thread_id);
    }

    pub(crate) fn get(&self, thread_id: &str) -> Option<VisibleRoot> {
        self.threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(thread_id)
            .cloned()
    }
}

/// Root entries rendered completely (not truncated or omitted) in the latest
/// project World State, keyed by entry ID with the alias and revision shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VisibleRoot {
    pub(crate) project_revision: u64,
    entries: HashMap<String, (String, u64)>,
}

impl VisibleRoot {
    pub(crate) fn new(project_revision: u64) -> Self {
        Self {
            project_revision,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, entry_id: String, alias: String, revision: u64) {
        self.entries.insert(entry_id, (alias, revision));
    }

    /// Returns the alias only when this exact entry revision was shown in full.
    pub(crate) fn alias_for(&self, entry_id: &str, revision: u64) -> Option<&str> {
        self.entries
            .get(entry_id)
            .filter(|(_, shown_revision)| *shown_revision == revision)
            .map(|(alias, _)| alias.as_str())
    }
}
