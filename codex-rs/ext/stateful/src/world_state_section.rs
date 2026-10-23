//! Adapts sections whose bounded comparison snapshot is captured for one sampling step.

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use serde_json::Value;

/// Retains the captured snapshot even when the section emits no model-visible update.
/// Producers enforce their existing snapshot and fragment limits before calling this.
pub(super) fn fixed_world_state_section(
    id: &'static str,
    snapshot: Value,
    render_diff: impl for<'a> Fn(PreviousWorldStateSection<'a>) -> Option<RenderedWorldStateFragment>
    + Send
    + Sync
    + 'static,
) -> WorldStateSectionContribution {
    WorldStateSectionContribution::new(id, move |previous| {
        (Some(snapshot.clone()), render_diff(previous))
    })
}
