//! Correction classification uses expanded composer text while retaining literal-mode intent.
use super::ChatComposer;

impl ChatComposer {
    pub(super) fn expanded_memory_correction(&self) -> bool {
        let text = self.current_text();
        if text.starts_with(' ') {
            return false;
        }
        let (expanded, _) = Self::expand_pending_pastes(
            &text, self.current_text_elements(), &self.draft.pending_pastes,
        );
        self.slash_input().is_memory_correction(&expanded)
    }
}
