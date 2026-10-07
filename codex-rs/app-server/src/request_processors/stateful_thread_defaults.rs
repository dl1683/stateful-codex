//! Product defaults applied to every thread that has a Stateful project selected.

use codex_core::config::Config;
use codex_features::Feature;

/// Turns on the defaults a Stateful thread runs with before its runtime is created. Today
/// that is line-ending preservation for `apply_patch`, so an edit to a CRLF file keeps the
/// file's endings instead of mixing in LF lines. A managed constraint that forbids a default
/// wins; the thread then keeps its configured behaviour.
pub(super) fn apply_stateful_thread_defaults(config: &mut Config) {
    if let Err(error) = config
        .features
        .enable(Feature::ApplyPatchPreserveLineEndings)
    {
        tracing::warn!(%error, "Stateful thread keeps apply_patch line-ending normalization");
    }
}

#[cfg(test)]
#[path = "stateful_thread_defaults_tests.rs"]
mod tests;
