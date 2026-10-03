//! The session's view of project memory: a cached summary for the passive footer and
//! `/status`, the journal watermark each visited project had when this TUI session first read
//! it (so the exit receipt counts the changes in this session's threads after it), and the
//! dated recap shown once on return. Every number comes from `statefulMemory/summary`;
//! notifications only say when to read it again.

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

/// Longest one summary read may take before its view is marked unavailable.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Longest the exit receipt waits for all its reads.
const EXIT_READ_TIMEOUT: Duration = Duration::from_secs(4);

/// Shared, cheaply cloned memory status of this TUI session.
#[derive(Clone, Default)]
pub(crate) struct MemoryStatus(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    /// What each thread this session attached to currently belongs to.
    threads: HashMap<ThreadId, ThreadMemory>,
    /// Per project: the journal sequence when this session first read it (sequences are
    /// counted per project) and every thread of this session that belonged to it.
    projects: BTreeMap<String, Visited>,
    recap_shown: bool,
    /// Per thread, bumped whenever its binding changes; a read started before is obsolete.
    generations: HashMap<ThreadId, u64>,
    /// Some project's memory could not be read when needed, so counts may be incomplete.
    incomplete: bool,
    /// The newest refresh request; a running refresh takes it before finishing.
    target: Option<Target>,
    refreshing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ThreadMemory {
    /// The thread has no Stateful project.
    NotStateful,
    /// Its project memory could not be read; tried again on the next activation.
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

impl State {
    /// Records that `thread_id` now belongs to `project_id`, whose journal head is `head`.
    fn bind(&mut self, thread_id: ThreadId, project_id: String, head: u64) {
        self.bump(thread_id);
        let visited = self.projects.entry(project_id.clone()).or_insert(Visited {
            watermark: head,
            threads: BTreeSet::new(),
        });
        visited.threads.insert(thread_id.to_string());
        self.threads
            .insert(thread_id, ThreadMemory::Project(project_id));
    }

    fn bump(&mut self, thread_id: ThreadId) {
        *self.generations.entry(thread_id).or_default() += 1;
    }

    fn generation(&self, thread_id: ThreadId) -> u64 {
        self.generations
            .get(&thread_id)
            .copied()
            .unwrap_or_default()
    }

    /// The project, watermark and participating threads `thread_id` is counted with.
    fn scope_of(&self, thread_id: ThreadId) -> Option<(String, u64, Vec<String>)> {
        let Some(ThreadMemory::Project(project)) = self.threads.get(&thread_id) else {
            return None;
        };
        let visited = self.projects.get(project)?;
        Some((
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

    /// A thread became active (or its project changed). Its project's watermark is taken on
    /// first use (before any turn of this session in it can save), then the status is shown
    /// and, once per session, the return recap.
    pub(crate) async fn attach(
        &self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        app_event_tx: AppEventSender,
    ) {
        // A thread without a project is read again each time: it may have been given one
        // while this client was not listening.
        let (known, generation) = {
            let state = self.lock();
            (
                matches!(
                    state.threads.get(&thread_id),
                    Some(ThreadMemory::Project(_))
                ),
                state.generation(thread_id),
            )
        };
        if !known {
            let read = summary(
                &handle, thread_id, /*since*/ None, /*threads*/ None,
            )
            .await;
            let mut state = self.lock();
            if state.generation(thread_id) != generation {
                // The binding changed while reading; the newer activation decides.
                return;
            }
            match read {
                Ok(response) => {
                    state.bind(thread_id, response.project_id, response.latest_sequence);
                }
                Err(SummaryError::NotStateful) => {
                    state.threads.insert(thread_id, ThreadMemory::NotStateful);
                }
                Err(SummaryError::Failed) => {
                    state.incomplete = true;
                    state.threads.insert(thread_id, ThreadMemory::Unavailable);
                }
            }
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
                if let Ok(Ok(recap)) =
                    tokio::time::timeout(READ_TIMEOUT, recap(&handle, thread_id)).await
                {
                    app_event_tx.send(AppEvent::StatefulMemoryRecap {
                        thread_id,
                        recap: Box::new(recap),
                    });
                }
            });
        }
    }

    /// The thread's project was assigned or changed: forget its binding, keeping what it did
    /// in its earlier project, so the next activation reads its current project.
    pub(crate) fn project_changed(&self, thread_id: ThreadId) {
        let mut state = self.lock();
        state.bump(thread_id);
        state.threads.remove(&thread_id);
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
                let (target, (project, since, threads), generation) = {
                    let mut state = status.lock();
                    let Some(target) = state.target.take() else {
                        state.refreshing = false;
                        break;
                    };
                    let Some(scope) = state.scope_of(target.thread_id) else {
                        continue;
                    };
                    let generation = state.generation(target.thread_id);
                    (target, scope, generation)
                };
                let read =
                    summary(&target.handle, target.thread_id, Some(since), Some(threads)).await;
                {
                    // The thread's binding changed while reading: this answer is obsolete.
                    let mut state = status.lock();
                    if state.generation(target.thread_id) != generation {
                        if state.target.is_none() {
                            state.target = Some(target);
                        }
                        continue;
                    }
                }
                match read {
                    Ok(response) if response.project_id == project => {
                        let partial = status.lock().incomplete;
                        target.app_event_tx.send(AppEvent::StatefulMemoryStatus {
                            thread_id: target.thread_id,
                            counts: response.counts,
                            session: response.since,
                            partial,
                        });
                    }
                    // The thread moved to another project: count it there from now on.
                    Ok(response) => {
                        let mut state = status.lock();
                        state.incomplete = true;
                        state.bind(
                            target.thread_id,
                            response.project_id,
                            response.latest_sequence,
                        );
                        if state.target.is_none() {
                            state.target = Some(target);
                        }
                    }
                    Err(_) => target
                        .app_event_tx
                        .send(AppEvent::StatefulMemoryUnavailable {
                            thread_id: target.thread_id,
                        }),
                }
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
                // A project is read through a thread that still belongs to it.
                let current = state.threads.iter().find_map(|(thread, memory)| {
                    matches!(memory, ThreadMemory::Project(other) if other == project)
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
                matches!(state.threads.get(&thread), Some(ThreadMemory::Project(_)))
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
