//! Bounded, serialized project index operations.
//!
//! A blocking scan cannot be stopped by dropping the future that awaits it, so an index
//! operation runs in its own task that owns the project's single index permit until the
//! operation actually exits. The caller waits at most its foreground deadline: past it,
//! the caller is told the work is still pending, never that it was terminated, and no
//! second operation for the project can start until the first one has exited. A
//! background ceiling signals cooperative cancellation, so abandoned work stops at the
//! scanner's next file boundary instead of running unbounded. Nothing here is a
//! session-wide failure state: once the permit is released, a later request retries.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use codex_project_intelligence::IndexCancellation;
use tokio::sync::Semaphore;

/// Foreground deadline for indexing one explicitly named source file.
pub(crate) const EXPLICIT_FILE_DEADLINE: Duration = Duration::from_secs(2);
/// Foreground deadline for building a never-built project index during a query.
pub(crate) const PROJECT_INDEX_DEADLINE: Duration = Duration::from_secs(20);
/// Longest an operation may keep running after its caller stopped waiting.
const BACKGROUND_CEILING: Duration = Duration::from_secs(120);

/// How a bounded index operation ended for its caller.
#[derive(Debug, PartialEq)]
pub(crate) enum IndexOperation<T> {
    /// The operation exited within the caller's deadline.
    Finished(T),
    /// This or an earlier operation for the project is still running past the deadline.
    /// It keeps the project's permit until it exits; a later request may retry.
    Pending,
    /// The operation's task panicked or was aborted.
    Failed(String),
}

/// One index permit per project, shared by every thread of this process.
#[derive(Clone, Default)]
pub(crate) struct IndexGates {
    gates: Arc<std::sync::Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl IndexGates {
    fn gate(&self, project_id: &str) -> Arc<Semaphore> {
        self.gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(project_id.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .clone()
    }

    /// Runs `operation` under the project's index permit, waiting at most `deadline` in
    /// total for the permit and the operation.
    pub(crate) async fn run<T, F>(
        &self,
        project_id: &str,
        deadline: Duration,
        operation: impl FnOnce(IndexCancellation) -> F,
    ) -> IndexOperation<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.run_with_ceiling(project_id, deadline, BACKGROUND_CEILING, operation)
            .await
    }

    async fn run_with_ceiling<T, F>(
        &self,
        project_id: &str,
        deadline: Duration,
        background_ceiling: Duration,
        operation: impl FnOnce(IndexCancellation) -> F,
    ) -> IndexOperation<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let started = Instant::now();
        let permit =
            match tokio::time::timeout(deadline, self.gate(project_id).acquire_owned()).await {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) | Err(_) => return IndexOperation::Pending,
            };
        let cancellation = IndexCancellation::default();
        let work = operation(cancellation.clone());
        let task = tokio::spawn(async move {
            let _permit = permit;
            let watchdog = tokio::spawn(async move {
                tokio::time::sleep(background_ceiling).await;
                cancellation.cancel();
            });
            // The permit is released only when the work itself has exited.
            let output = work.await;
            watchdog.abort();
            output
        });
        match tokio::time::timeout(deadline.saturating_sub(started.elapsed()), task).await {
            Ok(Ok(output)) => IndexOperation::Finished(output),
            Ok(Err(error)) => IndexOperation::Failed(error.to_string()),
            Err(_) => IndexOperation::Pending,
        }
    }
}

#[cfg(test)]
#[path = "index_gate_tests.rs"]
mod tests;
