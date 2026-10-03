//! The session's view of project memory: a cached summary for the passive footer and
//! `/status`, the journal watermark each visited project had when this TUI session first read
//! it (so the exit receipt counts the changes in this session's threads after it).
//! Every number comes from `statefulMemory/summary`; notifications only say when to read it
//! again.
//!
//! Every attachment of a thread (activation, reconnect, project change) gets a new generation.
//! Results carry the generation they were requested under and are applied only while it is
//! still current, so an answer about a former project or connection never reaches the view.
//! A binding is recorded only by the reading its attachment started, under the same lock
//! that application checks, so an obsolete reading can neither bind nor be shown.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulMemoryChangeTotals;
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

/// Longest one summary read may take before its view is marked unavailable.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Longest the exit receipt waits for all its reads.
const EXIT_READ_TIMEOUT: Duration = Duration::from_secs(4);

/// Shared, cheaply cloned memory status of this TUI session.
#[derive(Clone, Default)]
pub(crate) struct MemoryStatus(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    /// What each thread this session attached to belongs to now, and under which attachment.
    threads: HashMap<ThreadId, Binding>,
    /// Per project: the journal sequence when this session first read it (sequences are
    /// counted per project) and every thread of this session that belonged to it.
    projects: BTreeMap<String, Visited>,
    /// Source of attachment generations; never reused.
    next_generation: u64,
    /// Some project's memory could not be read when needed, so counts may be incomplete.
    incomplete: bool,
    /// The newest refresh request; a running refresh takes it before finishing.
    target: Option<Target>,
    refreshing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    generation: u64,
    memory: ThreadMemory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ThreadMemory {
    /// Being read for this attachment.
    Checking,
    /// The thread has no Stateful project.
    NotStateful,
    /// Its project memory could not be read for this attachment.
    Unavailable,
    /// The thread's current project.
    Project(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Visited {
    watermark: u64,
    threads: BTreeSet<String>,
}

#[derive(Clone)]
struct Target {
    handle: AppServerRequestHandle,
    thread_id: ThreadId,
    app_event_tx: AppEventSender,
}

/// What reading a thread's binding found.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AttachResult {
    Project { project_id: String, head: u64 },
    NotStateful,
    Failed,
}

/// What the app shows after an attachment, when it is still current.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AttachOutcome {
    /// Read the summary for the view.
    Refresh,
    /// The thread has no project memory: show none.
    Clear,
    /// Its memory could not be read.
    Unavailable,
    /// A newer attachment replaced this one; show nothing from it.
    Obsolete,
}

impl State {
    /// Starts a new attachment of `thread_id`; anything requested before is now obsolete.
    fn begin_attach(&mut self, thread_id: ThreadId) -> u64 {
        self.next_generation += 1;
        let generation = self.next_generation;
        self.threads.insert(
            thread_id,
            Binding {
                generation,
                memory: ThreadMemory::Checking,
            },
        );
        generation
    }

    /// Records what an attachment found, if it is still the thread's current attachment.
    fn finish_attach(
        &mut self,
        thread_id: ThreadId,
        generation: u64,
        result: AttachResult,
    ) -> AttachOutcome {
        if !self.is_current(thread_id, generation) {
            return AttachOutcome::Obsolete;
        }
        let (memory, outcome) = match result {
            AttachResult::Project { project_id, head } => {
                let visited = self.projects.entry(project_id.clone()).or_insert(Visited {
                    watermark: head,
                    threads: BTreeSet::new(),
                });
                visited.threads.insert(thread_id.to_string());
                (ThreadMemory::Project(project_id), AttachOutcome::Refresh)
            }
            // Earlier participation in a project stays in `projects` as history.
            AttachResult::NotStateful => (ThreadMemory::NotStateful, AttachOutcome::Clear),
            AttachResult::Failed => {
                self.incomplete = true;
                (ThreadMemory::Unavailable, AttachOutcome::Unavailable)
            }
        };
        self.threads
            .insert(thread_id, Binding { generation, memory });
        outcome
    }

    /// Whether `generation` is still the thread's current attachment.
    fn is_current(&self, thread_id: ThreadId, generation: u64) -> bool {
        self.threads
            .get(&thread_id)
            .is_some_and(|binding| binding.generation == generation)
    }

    /// The current attachment and its project's watermark and participating threads.
    fn scope_of(&self, thread_id: ThreadId) -> Option<(u64, String, u64, Vec<String>)> {
        let binding = self.threads.get(&thread_id)?;
        let ThreadMemory::Project(project) = &binding.memory else {
            return None;
        };
        let visited = self.projects.get(project)?;
        Some((
            binding.generation,
            project.clone(),
            visited.watermark,
            visited.threads.iter().cloned().collect(),
        ))
    }
}

impl MemoryStatus {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A thread became active, reconnected or changed project: read its binding again (a
    /// cached project may have been unlinked meanwhile), then show its memory or clear it.
    /// The session's first attachment is read before returning, so its project's watermark
    /// is taken before any turn of this session can save; later ones are read in the
    /// background so a slow or stalled server never holds the app's event loop.
    pub(crate) async fn attach(
        &self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        app_event_tx: AppEventSender,
    ) {
        let (first, generation) = {
            let mut state = self.lock();
            let first = state.threads.is_empty();
            (first, state.begin_attach(thread_id))
        };
        let read = self
            .clone()
            .read_binding(handle, thread_id, generation, app_event_tx);
        if first {
            read.await;
        } else {
            tokio::spawn(read);
        }
    }

    async fn read_binding(
        self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        generation: u64,
        app_event_tx: AppEventSender,
    ) {
        let result = match summary(
            &handle, thread_id, /*since*/ None, /*threads*/ None,
        )
        .await
        {
            Ok(response) => AttachResult::Project {
                project_id: response.project_id,
                head: response.latest_sequence,
            },
            Err(SummaryError::NotStateful) => AttachResult::NotStateful,
            Err(SummaryError::Failed) => AttachResult::Failed,
        };
        // Recorded only if no newer attachment of the thread began meanwhile.
        let outcome = self.lock().finish_attach(thread_id, generation, result);
        match outcome {
            AttachOutcome::Refresh => self.refresh(handle, thread_id, app_event_tx),
            AttachOutcome::Clear => app_event_tx.send(AppEvent::StatefulMemoryCleared {
                thread_id,
                generation,
            }),
            AttachOutcome::Unavailable => app_event_tx.send(AppEvent::StatefulMemoryUnavailable {
                thread_id,
                generation,
            }),
            AttachOutcome::Obsolete => {}
        }
    }

    /// The thread's project was assigned or changed: its current attachment is obsolete.
    pub(crate) fn project_changed(&self, thread_id: ThreadId) {
        self.lock().begin_attach(thread_id);
    }

    /// The connection was replaced: every attachment made through the old one is obsolete.
    pub(crate) fn connection_changed(&self) {
        let mut state = self.lock();
        let threads = state.threads.keys().copied().collect::<Vec<_>>();
        for thread_id in threads {
            state.begin_attach(thread_id);
        }
        state.target = None;
    }

    /// Whether a result requested under `generation` may still be applied to `thread_id`.
    /// Called on the app's event loop, where bindings change, right before applying it.
    pub(crate) fn accepts(&self, thread_id: ThreadId, generation: u64) -> bool {
        self.lock().is_current(thread_id, generation)
    }

    /// Reads the summary again in the background. Requests made while one runs collapse
    /// into one more read, for the newest thread and connection; a read that fails or takes
    /// too long marks the view unavailable instead of leaving old numbers as current.
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
                let (target, (generation, project, since, threads), partial) = {
                    let mut state = status.lock();
                    let Some(target) = state.target.take() else {
                        state.refreshing = false;
                        break;
                    };
                    let Some(scope) = state.scope_of(target.thread_id) else {
                        continue;
                    };
                    (target, scope, state.incomplete)
                };
                let read =
                    summary(&target.handle, target.thread_id, Some(since), Some(threads)).await;
                let thread_id = target.thread_id;
                let event = match read {
                    Ok(response) if response.project_id == project => {
                        AppEvent::StatefulMemoryStatus {
                            thread_id,
                            generation,
                            counts: response.counts,
                            session: response.since,
                            partial,
                        }
                    }
                    // The thread's project changed (perhaps unlinked) without a notification
                    // reaching this client: attach again to read the binding.
                    Ok(_) | Err(SummaryError::NotStateful) => {
                        AppEvent::StatefulMemoryProjectChanged { thread_id }
                    }
                    Err(SummaryError::Failed) => AppEvent::StatefulMemoryUnavailable {
                        thread_id,
                        generation,
                    },
                };
                target.app_event_tx.send(event);
            }
        });
    }

    /// The memory lines of the exit receipt: a last count, per visited project, of the
    /// changes in this session's threads, and what happens to the active thread's open run
    /// after the user quits. Empty outside a Stateful session.
    pub(crate) async fn exit_lines(
        &self,
        handle: &AppServerRequestHandle,
        active_thread: Option<ThreadId>,
        lifetime: ServerLifetime,
    ) -> Vec<String> {
        let (mut incomplete, reads, active_is_stateful) = {
            let state = self.lock();
            let mut incomplete = state.incomplete;
            let mut reads = Vec::new();
            for (project, visited) in &state.projects {
                // A project is read through a thread that still belongs to it; a project no
                // thread belongs to any longer is history this receipt cannot count.
                let current = state.threads.iter().find_map(|(thread, binding)| {
                    matches!(&binding.memory, ThreadMemory::Project(other) if other == project)
                        .then_some(*thread)
                });
                match current {
                    Some(thread) => reads.push((
                        project.clone(),
                        thread,
                        visited.watermark,
                        visited.threads.iter().cloned().collect::<Vec<_>>(),
                    )),
                    None => incomplete = true,
                }
            }
            let active_is_stateful = active_thread.is_some_and(|thread| {
                state
                    .threads
                    .get(&thread)
                    .is_some_and(|binding| matches!(binding.memory, ThreadMemory::Project(_)))
            });
            (incomplete, reads, active_is_stateful)
        };
        if reads.is_empty() && !incomplete {
            return Vec::new();
        }
        let read = async {
            let mut totals = StatefulMemoryChangeTotals::default();
            let mut complete = true;
            for (project, thread_id, since, threads) in &reads {
                match summary(handle, *thread_id, Some(*since), Some(threads.clone())).await {
                    Ok(StatefulMemorySummaryResponse {
                        project_id,
                        since: Some(project_totals),
                        ..
                    }) if &project_id == project => add(&mut totals, &project_totals),
                    Ok(_) | Err(_) => complete = false,
                }
            }
            let run = match active_thread.filter(|_| active_is_stateful) {
                Some(thread_id) => run(handle, thread_id).await,
                None => None,
            };
            (totals, complete, run)
        };
        let (totals, run) = match tokio::time::timeout(EXIT_READ_TIMEOUT, read).await {
            Ok((totals, complete, run)) => {
                incomplete |= !complete;
                (Some(totals), run)
            }
            Err(_) => (None, None),
        };
        let coverage = if incomplete {
            ExitCoverage::Partial
        } else {
            ExitCoverage::Complete
        };
        crate::memory_receipts::exit_lines(totals.as_ref(), coverage, run.as_ref(), lifetime)
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
    /// The read failed or took too long.
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
    let read = handle.request_typed(ClientRequest::StatefulMemorySummary {
        request_id: request_id("summary"),
        params: StatefulMemorySummaryParams {
            thread_id: thread_id.to_string(),
            since_sequence,
            thread_ids,
        },
    });
    match tokio::time::timeout(READ_TIMEOUT, read).await {
        Ok(Ok(response)) => Ok(response),
        // The app server's answer for a thread outside any project.
        Ok(Err(error)) if error.to_string().contains("has no project") => {
            Err(SummaryError::NotStateful)
        }
        Ok(Err(_)) | Err(_) => Err(SummaryError::Failed),
    }
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
