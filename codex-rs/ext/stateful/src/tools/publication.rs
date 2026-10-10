//! The Stateful publication fence. Every Stateful tool output reads or reports memory at one
//! moment, then waits (PostToolUse hooks, the rest of the model's response stream, earlier
//! parallel results) before the host records it into model history. If a user's Forget, Undo
//! or correction commits in between, the waiting output can quote words that no longer apply
//! or report coverage that no longer holds.
//!
//! Each call captures the project's retirement generation before its handler runs. When the
//! host is about to record the result, the check takes the store's writer lock (bounded by
//! [`PUBLICATION_LOCK_TIMEOUT`]), compares the generation, and hands the host the lock as the
//! guard it holds only while recording. Forget, Undo and correction writers take the same
//! lock, in this or any other process, so a retirement either commits before the comparison
//! (the output is withheld) or after the output is recorded. A lock that cannot be taken in
//! time also withholds the output; nothing waits on it.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolPublicationCheck;
use codex_extension_api::ToolPublicationGuard;

use crate::services::ProjectIntelligenceServices;

/// Calls awaiting publication per tool instance. A call evicted from this bound has no
/// captured generation and is withheld.
const MAX_PENDING_CALLS: usize = 64;

/// Longest wait for the store's writer lock at publication. The lock is held only while one
/// result is recorded, so a longer wait means another writer is busy; the result is then
/// withheld rather than delayed.
const PUBLICATION_LOCK_TIMEOUT: Duration = Duration::from_secs(1);

/// What the model receives instead of a stale read.
const STALE_READ: &str = "memory changed while this result was pending (a Forget, Undo or correction committed after it was read, or memory could not be checked); it was withheld, with anything a hook derived from it. Re-run the read for current memory.";

/// What the model receives instead of a writer's stale successful output.
const STALE_COMMITTED_WRITE: &str = "this call's change was committed, but its output was withheld because memory changed while it was pending (a Forget, Undo or correction committed, or memory could not be checked). Do not repeat the call; re-read memory for the current state.";

/// What the model receives instead of a writer's stale error.
const STALE_FAILED_WRITE: &str = "this call's result was withheld because memory changed while it was pending (a Forget, Undo or correction committed, or memory could not be checked). It may already have taken effect: re-read memory before deciding whether to retry.";

/// Whether a tool can change stored state, which decides the withheld wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    Read,
    Write,
}

/// A finished call awaiting publication.
struct Pending {
    call_id: String,
    generation: Option<u64>,
    succeeded: bool,
}

/// A Stateful tool whose outputs are checked where the host records them.
pub(super) struct Fenced {
    inner: Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>,
    effect: Effect,
    project_id: String,
    services: ProjectIntelligenceServices,
    pending: Mutex<VecDeque<Pending>>,
}

impl Fenced {
    pub(super) fn new(
        inner: Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>,
        effect: Effect,
        project_id: String,
        services: ProjectIntelligenceServices,
    ) -> Self {
        Self {
            inner,
            effect,
            project_id,
            services,
            pending: Mutex::new(VecDeque::new()),
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, VecDeque<Pending>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Memory-bearing outputs are delivered only to direct model calls: a nested Code Mode cell
/// can hold a result before printing it, outside any recording the fence can order.
fn direct_only(exposure: ToolExposure) -> ToolExposure {
    match exposure {
        ToolExposure::Direct | ToolExposure::DirectModelOnly => ToolExposure::DirectModelOnly,
        ToolExposure::Deferred | ToolExposure::DeferredModelOnly => ToolExposure::DeferredModelOnly,
        ToolExposure::CodeModeOnly | ToolExposure::Hidden => ToolExposure::Hidden,
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for Fenced {
    fn tool_name(&self) -> codex_extension_api::ToolName {
        self.inner.tool_name()
    }

    fn spec(&self) -> codex_extension_api::ToolSpec {
        self.inner.spec()
    }

    fn exposure(&self) -> ToolExposure {
        direct_only(self.inner.exposure())
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.inner.supports_parallel_tool_calls()
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> ToolExecutorFuture<'a>
    where
        ToolCall<'call>: 'a,
    {
        Box::pin(async move {
            let call_id = call.call_id.clone();
            // Captured before the handler, so a retirement during the read also counts.
            let generation = match self.services.blackboard().await {
                Ok(store) => store.retirement_generation(&self.project_id).await.ok(),
                Err(_) => None,
            };
            let result = self.inner.handle(call).await;
            let mut pending = self.pending();
            if pending.len() == MAX_PENDING_CALLS {
                pending.pop_front();
            }
            pending.push_back(Pending {
                call_id,
                generation,
                succeeded: result.is_ok(),
            });
            result
        })
    }

    fn publication_check(&self, call_id: &str) -> Option<ToolPublicationCheck> {
        let finished = {
            let mut pending = self.pending();
            pending
                .iter()
                .position(|pending| pending.call_id == call_id)
                .and_then(|index| pending.remove(index))
        };
        let (generation, succeeded) = finished.map_or((None, false), |finished| {
            (finished.generation, finished.succeeded)
        });
        let withheld = match (self.effect, succeeded) {
            (Effect::Read, _) => STALE_READ,
            (Effect::Write, true) => STALE_COMMITTED_WRITE,
            (Effect::Write, false) => STALE_FAILED_WRITE,
        };
        let services = self.services.clone();
        let project_id = self.project_id.clone();
        Some(ToolPublicationCheck {
            withheld: withheld.to_string(),
            validate: Box::new(move || {
                Box::pin(async move {
                    let captured = generation?;
                    let store = services.blackboard().await.ok()?;
                    let mut fence = store
                        .acquire_completion_fence(PUBLICATION_LOCK_TIMEOUT)
                        .await
                        .ok()?;
                    let current = fence.retirement_generation(&project_id).await.ok()?;
                    (current == captured).then(|| Box::new(fence) as ToolPublicationGuard)
                })
            }),
        })
    }
}

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;
