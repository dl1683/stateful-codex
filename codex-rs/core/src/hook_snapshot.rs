//! The hook set a turn dispatches is fixed when its task starts.
//!
//! Hook changes published while a turn runs (a config reload, a trust change) apply from the
//! next turn. Publication records the host's "hooks configured" fact before it stores the new
//! hooks, so anything an extension observes about configured hooks during a turn covers every
//! hook that can still run in that turn, including the hooks around its later tool calls.

use std::sync::Arc;

use codex_hooks::Hooks;

use crate::session::session::Session;
use crate::session::turn_context::TurnContext;

/// The session hooks captured for one turn.
struct TurnHooks(Arc<Hooks>);

/// Fixes `turn`'s hook set to the session's current hooks.
pub(crate) fn pin_turn_hooks(sess: &Session, turn: &TurnContext) {
    turn.extension_data.insert(TurnHooks(sess.hooks()));
}

/// The hooks to dispatch in `turn`: its pinned set, or the session's current hooks for a
/// context no task started.
pub(crate) fn turn_hooks(sess: &Session, turn: &TurnContext) -> Arc<Hooks> {
    turn.extension_data
        .get::<TurnHooks>()
        .map_or_else(|| sess.hooks(), |pinned| Arc::clone(&pinned.0))
}
