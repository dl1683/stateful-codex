use std::collections::HashMap;

/// Consumes the core warning copy of each startup configuration warning once.
///
/// `codex exec` receives startup configuration warnings from the app-server
/// initialization channel and again when the core session starts. The first
/// copy is the canonical headless output; this bounded multiset recognizes the
/// second copy without suppressing later runtime warnings with the same text.
pub(crate) struct StartupWarningDeduper {
    remaining: HashMap<String, usize>,
}

impl StartupWarningDeduper {
    pub(crate) fn new(warnings: &[String]) -> Self {
        let mut remaining = HashMap::new();
        for warning in warnings {
            *remaining.entry(warning.clone()).or_default() += 1;
        }
        Self { remaining }
    }

    pub(crate) fn take_duplicate(&mut self, message: &str) -> bool {
        let remove = {
            let Some(remaining) = self.remaining.get_mut(message) else {
                return false;
            };
            *remaining -= 1;
            *remaining == 0
        };
        if remove {
            self.remaining.remove(message);
        }
        true
    }
}

#[cfg(test)]
#[path = "startup_warning_deduper_tests.rs"]
mod tests;
