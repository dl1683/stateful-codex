//! The session's view of project memory: a cached summary for the passive footer and
//! `/status`, the journal watermark each visited project had when this TUI session first read
//! it (so the exit receipt counts the changes in this session's threads after it), and the
//! dated recap shown once on return. Every number comes from `statefulMemory/summary`;
//! notifications only say when to read it again.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryRecapParams;
use codex_app_server_protocol::StatefulMemoryRecapResponse;
use codex_app_server_protocol::StatefulMemorySummaryParams;
use codex_app_server_protocol::StatefulMemorySummaryResponse;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_protocol::ThreadId;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::memory_receipts::ExitCoverage;
use crate::memory_receipts::ServerLifetime;

/// Longest the exit receipt waits for the final count.
const EXIT_READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Longest the first read (a project's watermark) may hold the event loop.
const WATERMARK_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared, cheaply cloned memory status of this TUI session.
#[derive(Clone, Default)]
pub(crate) struct MemoryStatus(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    /// What is known about each thread this session attached to.
    threads: HashMap<ThreadId, ThreadMemory>,
    /// Per project: the journal sequence when this session first read it. Journal sequences
    /// are counted per project, so each project keeps its own.
    watermarks: BTreeMap<String, u64>,
    recap_shown: bool,
    /// Some thread's memory could not be read when it became active, so the session's
    /// count is incomplete.
    incomplete: bool,
    /// The newest refresh request; a running refresh takes it before finishing.
    target: Option<Target>,
    refreshing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ThreadMemory {
    /// The thread has no Stateful project.
    NotStateful,
    /// Its project memory could not be read when the thread became active; changes made
    /// before a later successful read are not counted.
    Unavailable,
    /// The thread's project.
    Project(String),
}

#[derive(Clone)]
struct Target {
    handle: AppServerRequestHandle,
    thread_id: ThreadId,
    app_event_tx: AppEventSender,
}

impl State {
    /// The watermark and this session's threads of `thread_id`'s project.
    fn scope_of(&self, thread_id: ThreadId) -> Option<(u64, Vec<String>)> {
        let Some(ThreadMemory::Project(project)) = self.threads.get(&thread_id) else {
            return None;
        };
        let threads = self
            .threads
            .iter()
            .filter(
                |(_, memory)| matches!(memory, ThreadMemory::Project(other) if other == project),
            )
            .map(|(thread, _)| thread.to_string())
            .collect();
        Some((*self.watermarks.get(project)?, threads))
    }
}

impl MemoryStatus {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A thread became active. Its project's watermark is taken on first use (before any turn
    /// of this session in it can save), then the status is shown and, once per session, the
    /// return recap. A thread whose memory could not be read is tried again on its next
    /// activation, and the receipt says its count is incomplete.
    pub(crate) async fn attach(
        &self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        app_event_tx: AppEventSender,
    ) {
        let known = matches!(
            self.lock().threads.get(&thread_id),
            Some(ThreadMemory::NotStateful | ThreadMemory::Project(_))
        );
        if !known {
            let read = tokio::time::timeout(
                WATERMARK_READ_TIMEOUT,
                summary(
                    &handle, thread_id, /*since*/ None, /*threads*/ None,
                ),
            )
            .await;
            let mut state = self.lock();
            let memory = match read {
                Ok(Ok(response)) => {
                    // A thread first read after a failure starts counting only now.
                    state
                        .watermarks
                        .entry(response.project_id.clone())
                        .or_insert(response.latest_sequence);
                    ThreadMemory::Project(response.project_id)
                }
                Ok(Err(SummaryError::NotStateful)) => ThreadMemory::NotStateful,
                Ok(Err(SummaryError::Failed)) | Err(_) => ThreadMemory::Unavailable,
            };
            // Once unavailable, the count stays marked incomplete even after a later read.
            if memory == ThreadMemory::Unavailable {
                state.incomplete = true;
            }
            state.threads.insert(thread_id, memory);
        }
        let memory = self.lock().threads.get(&thread_id).cloned();
        match memory {
            Some(ThreadMemory::NotStateful) | None => return,
            Some(ThreadMemory::Unavailable) => {
                app_event_tx.send(AppEvent::StatefulMemoryUnavailable { thread_id });
                return;
            }
            Some(ThreadMemory::Project(_)) => {}
        }
        let show_recap = !std::mem::replace(&mut self.lock().recap_shown, true);
        self.refresh(handle.clone(), thread_id, app_event_tx.clone());
        if show_recap {
            tokio::spawn(async move {
                if let Ok(recap) = recap(&handle, thread_id).await {
                    app_event_tx.send(AppEvent::StatefulMemoryRecap {
                        thread_id,
                        recap: Box::new(recap),
                    });
                }
            });
        }
    }

    /// Reads the summary again in the background. Requests made while one runs collapse
    /// into one more read, for the newest thread and connection.
    pub(crate) fn refresh(
        &self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        app_event_tx: AppEventSender,
    ) {
        {
            let mut state = self.lock();
            if state.scope_of(thread_id).is_none() {
                return;
            }
            state.target = Some(Target {
                handle,
                thread_id,
                app_event_tx,
            });
            if std::mem::replace(&mut state.refreshing, true) {
                return;
            }
        }
        let status = self.clone();
        tokio::spawn(async move {
            loop {
                let (target, (since, threads)) = {
                    let mut state = status.lock();
                    let Some(target) = state.target.take() else {
                        state.refreshing = false;
                        break;
                    };
                    let Some(scope) = state.scope_of(target.thread_id) else {
                        continue;
                    };
                    (target, scope)
                };
                if let Ok(response) =
                    summary(&target.handle, target.thread_id, Some(since), Some(threads)).await
                {
                    target.app_event_tx.send(AppEvent::StatefulMemoryStatus {
                        thread_id: target.thread_id,
                        counts: response.counts,
                        session: response.since,
                    });
                }
            }
        });
    }

    /// The memory lines of the exit receipt: a last count, per visited project, of the
    /// changes in this session's threads, and what happens to an open run after the user
    /// quits. Empty outside a Stateful session.
    pub(crate) async fn exit_lines(
        &self,
        handle: &AppServerRequestHandle,
        active_thread: Option<ThreadId>,
        lifetime: ServerLifetime,
    ) -> Vec<String> {
        let (incomplete, scopes) = {
            let state = self.lock();
            // One count per visited project, through any of its threads.
            let mut scopes: BTreeMap<String, (ThreadId, u64, Vec<String>)> = BTreeMap::new();
            for (thread, memory) in &state.threads {
                if let ThreadMemory::Project(project) = memory
                    && !scopes.contains_key(project)
                    && let Some((since, threads)) = state.scope_of(*thread)
                {
                    scopes.insert(project.clone(), (*thread, since, threads));
                }
            }
            (state.incomplete, scopes)
        };
        if scopes.is_empty() && !incomplete {
            return Vec::new();
        }
        let read = async {
            let mut totals = StatefulMemoryChangeTotals::default();
            let mut coverage = if incomplete {
                ExitCoverage::Partial
            } else {
                ExitCoverage::Complete
            };
            for (thread_id, since, threads) in scopes.values() {
                match summary(handle, *thread_id, Some(*since), Some(threads.clone())).await {
                    Ok(StatefulMemorySummaryResponse {
                        since: Some(project_totals),
                        ..
                    }) => add(&mut totals, &project_totals),
                    Ok(_) | Err(_) => coverage = ExitCoverage::Partial,
                }
            }
            let run = match active_thread
                .filter(|thread| scopes.values().any(|(other, _, _)| other == thread))
            {
                Some(thread_id) => run(handle, thread_id).await,
                None => None,
            };
            (totals, coverage, run)
        };
        match tokio::time::timeout(EXIT_READ_TIMEOUT, read).await {
            Ok((totals, coverage, run)) => {
                crate::memory_receipts::exit_lines(Some(&totals), coverage, run.as_ref(), lifetime)
            }
            Err(_) => crate::memory_receipts::exit_lines(
                /*session*/ None,
                ExitCoverage::Partial,
                /*run*/ None,
                lifetime,
            ),
        }
    }
}

fn add(totals: &mut StatefulMemoryChangeTotals, more: &StatefulMemoryChangeTotals) {
    totals.saved += more.saved;
    totals.commits_remembered += more.commits_remembered;
    totals.promoted += more.promoted;
    totals.corrected += more.corrected;
    totals.forgotten += more.forgotten;
    totals.invalidated += more.invalidated;
    totals.scopes_ended += more.scopes_ended;
    totals.capture_incomplete += more.capture_incomplete;
}

/// Why a summary could not be read.
enum SummaryError {
    /// The thread has no project, so it has no project memory.
    NotStateful,
    Failed,
}

fn request_id(action: &str) -> RequestId {
    RequestId::String(format!(
        "stateful-tui-memory-status-{action}-{}",
        uuid::Uuid::new_v4()
    ))
}

async fn summary(
    handle: &AppServerRequestHandle,
    thread_id: ThreadId,
    since_sequence: Option<u64>,
    thread_ids: Option<Vec<String>>,
) -> Result<StatefulMemorySummaryResponse, SummaryError> {
    handle
        .request_typed(ClientRequest::StatefulMemorySummary {
            request_id: request_id("summary"),
            params: StatefulMemorySummaryParams {
                thread_id: thread_id.to_string(),
                since_sequence,
                thread_ids,
            },
        })
        .await
        .map_err(|error| {
            // The app server's answer for a thread outside any project.
            if error.to_string().contains("has no project") {
                SummaryError::NotStateful
            } else {
                SummaryError::Failed
            }
        })
}

async fn recap(
    handle: &AppServerRequestHandle,
    thread_id: ThreadId,
) -> Result<StatefulMemoryRecapResponse, String> {
    handle
        .request_typed(ClientRequest::StatefulMemoryRecap {
            request_id: request_id("recap"),
            params: StatefulMemoryRecapParams {
                thread_id: thread_id.to_string(),
            },
        })
        .await
        .map_err(|error| error.to_string())
}

async fn run(handle: &AppServerRequestHandle, thread_id: ThreadId) -> Option<StatefulRun> {
    handle
        .request_typed::<StatefulRunReadResponse>(ClientRequest::StatefulRunRead {
            request_id: request_id("run-read"),
            params: StatefulRunReadParams {
                run_id: None,
                thread_id: Some(thread_id.to_string()),
            },
        })
        .await
        .ok()
        .and_then(|response| response.run)
}

#[cfg(test)]
#[path = "memory_status_tests.rs"]
mod tests;
