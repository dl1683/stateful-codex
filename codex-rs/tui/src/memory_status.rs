//! The session's view of project memory: a cached summary for the passive footer and
//! `/status`, the journal watermark this TUI session started at (so the exit receipt counts
//! only this session's changes, in this session's threads), and the dated recap shown once
//! on return. Every number comes from `statefulMemory/summary`; notifications only say when
//! to read it again.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
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

/// Longest the exit receipt waits for the final count.
const EXIT_READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Longest the first read (the session's watermark) may hold the event loop.
const WATERMARK_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared, cheaply cloned memory status of this TUI session.
#[derive(Clone, Default)]
pub(crate) struct MemoryStatus(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    /// The journal sequence when this session first read project memory.
    session_start: Option<u64>,
    /// Threads this session attached to; its changes are counted in these.
    threads: Vec<String>,
    /// The newest summary and the thread it was read for.
    latest: Option<(ThreadId, StatefulMemorySummaryResponse)>,
    recap_shown: bool,
    refreshing: bool,
    refresh_again: bool,
}

impl MemoryStatus {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A thread became active: count its changes from now on, take the session's watermark
    /// on first use (before any turn of this session can save), then show the status and,
    /// once per session, the return recap.
    pub(crate) async fn attach(
        &self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        app_event_tx: AppEventSender,
    ) {
        let first = {
            let mut state = self.lock();
            let id = thread_id.to_string();
            if !state.threads.contains(&id) {
                state.threads.push(id);
            }
            state.session_start.is_none()
        };
        if first {
            let read = tokio::time::timeout(
                WATERMARK_READ_TIMEOUT,
                summary(
                    &handle, thread_id, /*since*/ None, /*threads*/ None,
                ),
            )
            .await;
            match read {
                Ok(Ok(response)) => {
                    let mut state = self.lock();
                    state.session_start.get_or_insert(response.latest_sequence);
                }
                // Not a Stateful project (or no memory yet): nothing to show.
                Ok(Err(_)) | Err(_) => return,
            }
        }
        let show_recap = {
            let mut state = self.lock();
            !std::mem::replace(&mut state.recap_shown, true)
        };
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

    /// Reads the summary again in the background; overlapping requests collapse into one
    /// more read after the current one.
    pub(crate) fn refresh(
        &self,
        handle: AppServerRequestHandle,
        thread_id: ThreadId,
        app_event_tx: AppEventSender,
    ) {
        {
            let mut state = self.lock();
            if state.session_start.is_none() {
                return;
            }
            if state.refreshing {
                state.refresh_again = true;
                return;
            }
            state.refreshing = true;
        }
        let status = self.clone();
        tokio::spawn(async move {
            loop {
                let (since, threads) = status.session_scope();
                if let Ok(response) = summary(&handle, thread_id, since, Some(threads)).await {
                    let (counts, session) = (response.counts, response.since);
                    status.lock().latest = Some((thread_id, response));
                    app_event_tx.send(AppEvent::StatefulMemoryStatus {
                        thread_id,
                        counts,
                        session,
                    });
                }
                let mut state = status.lock();
                if std::mem::take(&mut state.refresh_again) {
                    continue;
                }
                state.refreshing = false;
                break;
            }
        });
    }

    fn session_scope(&self) -> (Option<u64>, Vec<String>) {
        let state = self.lock();
        (state.session_start, state.threads.clone())
    }

    /// The memory lines of the exit receipt: one last count of this session's changes, and
    /// what happens to an open run after the user quits. Empty outside a Stateful session.
    pub(crate) async fn exit_lines(&self, handle: &AppServerRequestHandle) -> Vec<String> {
        let (since, threads) = self.session_scope();
        let Some(thread_id) = self.lock().latest.as_ref().map(|(thread_id, _)| *thread_id) else {
            return Vec::new();
        };
        let read = async {
            let totals = summary(handle, thread_id, since, Some(threads))
                .await
                .ok()
                .and_then(|response| response.since);
            let run = run(handle, thread_id).await;
            (totals, run)
        };
        match tokio::time::timeout(EXIT_READ_TIMEOUT, read).await {
            Ok((totals, run)) => crate::memory_receipts::exit_lines(totals.as_ref(), run.as_ref()),
            Err(_) => vec![
                "Project memory: this session's changes could not be counted in time; /memory shows what is saved."
                    .to_string(),
            ],
        }
    }
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
) -> Result<StatefulMemorySummaryResponse, String> {
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
        .map_err(|error| error.to_string())
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
