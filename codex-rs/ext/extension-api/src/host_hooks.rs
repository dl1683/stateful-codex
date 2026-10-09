//! Process-wide host fact: whether lifecycle hooks able to run a command or tool were ever
//! configured in this process. Hooks run outside any tool call (around tool calls, at turn
//! boundaries, after turns), so an extension that certifies an action-free run must know
//! whether any could have run. The fact is sticky: hooks configured and later removed may
//! already have run.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static HOST_HOOKS_CONFIGURED: AtomicBool = AtomicBool::new(false);

/// Called by the host whenever it installs hooks that could run, before any of them can.
pub fn record_host_hooks_configured() {
    HOST_HOOKS_CONFIGURED.store(true, Ordering::SeqCst);
}

/// Whether this process has had hooks configured that could run.
pub fn host_hooks_configured() -> bool {
    HOST_HOOKS_CONFIGURED.load(Ordering::SeqCst)
}
