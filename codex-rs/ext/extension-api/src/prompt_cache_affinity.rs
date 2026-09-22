use std::sync::RwLock;

/// Mutable cache-routing affinity owned by the host for one model thread.
///
/// The value affects prompt-prefix reuse only. It does not replace the thread or
/// session identity recorded in model metadata. Hosts should use a scope whose
/// stable model-visible prefix is intentionally shared, and update it whenever
/// that scope changes.
#[derive(Debug, Default)]
pub struct PromptCacheAffinity {
    key: RwLock<Option<String>>,
}

impl PromptCacheAffinity {
    /// Creates an affinity with an active cache-routing key.
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: RwLock::new(Some(key.into())),
        }
    }

    /// Returns the current cache-routing key, or `None` for thread-local routing.
    pub fn key(&self) -> Option<String> {
        self.key
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Replaces the current cache-routing key.
    pub fn set(&self, key: impl Into<String>) {
        *self
            .key
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(key.into());
    }

    /// Clears shared affinity so the caller can fall back to thread-local routing.
    pub fn clear(&self) {
        *self
            .key
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}
