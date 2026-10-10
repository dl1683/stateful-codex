//! The Stateful publication fence. Memory-bearing tool outputs are read from a snapshot, then
//! the host may hold them (PostToolUse hooks) before the model sees them. If a user's Forget,
//! Undo or correction commits in between, the held output can quote words that no longer
//! apply. Each fenced call captures the project's retirement generation before its read; at
//! publication, after every hook, an output whose generation is no longer current is replaced
//! by a bounded notice instead of being published.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolPublicationFuture;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;

/// Calls awaiting publication per tool instance. A call evicted from this bound has no
/// captured generation and is treated as stale.
const MAX_PENDING_CALLS: usize = 64;

/// What the model receives instead of a stale memory output.
const STALE_NOTICE: &str = "memory changed while this result was pending (a Forget, Undo or correction committed after it was read); it was withheld. Re-run the read for current memory.";

/// A memory-bearing tool whose outputs are revalidated at publication.
pub(super) struct Fenced {
    inner: Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>,
    project_id: String,
    services: ProjectIntelligenceServices,
    pending: Mutex<VecDeque<(String, u64)>>,
}

impl Fenced {
    pub(super) fn new(
        inner: Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>,
        project_id: String,
        services: ProjectIntelligenceServices,
    ) -> Self {
        Self {
            inner,
            project_id,
            services,
            pending: Mutex::new(VecDeque::new()),
        }
    }

    async fn generation(&self) -> Option<u64> {
        let store = self.services.blackboard().await.ok()?;
        store.retirement_generation(&self.project_id).await.ok()
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, VecDeque<(String, u64)>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for Fenced {
    fn tool_name(&self) -> codex_extension_api::ToolName {
        self.inner.tool_name()
    }

    fn spec(&self) -> codex_extension_api::ToolSpec {
        self.inner.spec()
    }

    fn exposure(&self) -> codex_extension_api::ToolExposure {
        self.inner.exposure()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.inner.supports_parallel_tool_calls()
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> ToolExecutorFuture<'a>
    where
        ToolCall<'call>: 'a,
    {
        Box::pin(async move {
            // Captured before the read, so a retirement during the read also counts.
            if let Some(generation) = self.generation().await {
                let mut pending = self.pending();
                if pending.len() == MAX_PENDING_CALLS {
                    pending.pop_front();
                }
                pending.push_back((call.call_id.clone(), generation));
            }
            self.inner.handle(call).await
        })
    }

    fn revalidate_for_publication<'a>(
        &'a self,
        call_id: &'a str,
        output: Box<dyn ToolOutput>,
    ) -> ToolPublicationFuture<'a> {
        Box::pin(async move {
            let captured = {
                let mut pending = self.pending();
                pending
                    .iter()
                    .position(|(id, _)| id == call_id)
                    .and_then(|index| pending.remove(index))
                    .map(|(_, generation)| generation)
            };
            let current = self.generation().await;
            if captured.is_some() && captured == current {
                return output;
            }
            Box::new(JsonToolOutput::new(json!({ "unavailable": STALE_NOTICE })))
                as Box<dyn ToolOutput>
        })
    }
}
