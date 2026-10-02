//! Gathers the continuity record from the thread store's per-turn summaries (first user
//! message and final answer) of the selected project's recent top-level threads.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WorkflowMode;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredTurn;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::ThreadStore;

use crate::continuity::CapturedTurn;
use crate::continuity::ContinuityRecord;
use crate::continuity::LatestRun;
use crate::continuity::RunLabel;
use crate::conversation_summaries::is_top_level;
use crate::conversation_summaries::project_threads_params;
use crate::conversation_summaries::turn_status;
use crate::conversation_summaries::turn_texts;
use crate::conversation_summaries::turn_time_ms;

const MAX_THREADS: usize = 5;
/// Threads listed to find `MAX_THREADS` top-level ones among subagent threads.
const MAX_THREADS_LISTED: usize = 25;
const MAX_TURNS_PER_THREAD: usize = 8;
const MAX_TURNS: usize = 10;
/// A thread with this many runs or more gets an unknown run binding.
const MAX_RUNS_PER_THREAD: u32 = 20;

/// Reads the newest turn summaries of the project's top-level, unarchived threads, labelled
/// with the run inferred to own each turn, and the project's latest run.
pub(super) async fn gather_continuity(
    threads: &dyn ThreadStore,
    runtime: Option<&StatefulRunStore>,
    project_id: &str,
    current_thread_id: &str,
) -> ContinuityRecord {
    let mut more_turns = false;
    let mut unreadable_threads = 0;
    let mut history_unavailable = false;
    let mut turns = Vec::new();
    let listed = match threads
        .list_threads(project_threads_params(
            project_id,
            MAX_THREADS_LISTED,
            /*cursor*/ None,
        ))
        .await
    {
        Ok(page) => {
            more_turns |= page.next_cursor.is_some();
            page.items
        }
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to list project threads for continuity");
            history_unavailable = true;
            Vec::new()
        }
    };
    let top_level = listed.into_iter().filter(is_top_level).collect::<Vec<_>>();
    more_turns |= top_level.len() > MAX_THREADS;
    for thread in top_level.iter().take(MAX_THREADS) {
        let page = match threads
            .list_turns(ListTurnsParams {
                thread_id: thread.thread_id,
                include_archived: false,
                cursor: None,
                page_size: MAX_TURNS_PER_THREAD,
                sort_direction: SortDirection::Desc,
                items_view: StoredTurnItemsView::Summary,
            })
            .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::debug!(thread_id = %thread.thread_id, %error, "thread has no readable turn summaries");
                unreadable_threads += 1;
                continue;
            }
        };
        more_turns |= page.next_cursor.is_some();
        let thread_id = thread.thread_id.to_string();
        let runs = match runtime {
            Some(runtime) => runtime
                .runs_for_thread(&thread_id, MAX_RUNS_PER_THREAD)
                .await
                .ok()
                .filter(|runs| runs.len() < MAX_RUNS_PER_THREAD as usize),
            None => None,
        };
        let current_thread = thread_id == current_thread_id;
        turns.extend(
            page.turns
                .into_iter()
                .filter_map(|turn| captured_turn(thread, current_thread, runs.as_deref(), turn)),
        );
    }
    turns.sort_by_key(|turn| std::cmp::Reverse(turn.at_ms));
    if turns.len() > MAX_TURNS {
        turns.truncate(MAX_TURNS);
        more_turns = true;
    }
    ContinuityRecord {
        project_id: project_id.to_string(),
        captured_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            }),
        turns,
        more_turns,
        unreadable_threads,
        history_unavailable,
        latest_run: match runtime {
            Some(runtime) => latest_run(runtime, project_id).await,
            None => None,
        },
    }
}

async fn latest_run(runtime: &StatefulRunStore, project_id: &str) -> Option<LatestRun> {
    let run = match runtime.latest_project_run(project_id).await {
        Ok(run) => run?,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to load the latest Stateful run");
            return None;
        }
    };
    let next = match runtime.latest_obligation(&run.id).await {
        Ok(obligation) => {
            obligation.map_or_else(Vec::new, |obligation| obligation.value.packet.next)
        }
        Err(error) => {
            tracing::warn!(run_id = %run.id, %error, "failed to load the latest obligation");
            Vec::new()
        }
    };
    Some(LatestRun {
        id: run.id.to_string(),
        mode: match run.value.mode {
            WorkflowMode::Autonomous => "autonomous",
            WorkflowMode::Collaborative => "collaborative",
            WorkflowMode::Socratic => "socratic",
        },
        status: status_name(run.status),
        next,
        strategy: run.strategy,
    })
}

/// The run open when a turn started: created no later than that second and, if terminal,
/// last updated at or after it. `runs` is `None` when the run history is unusable.
fn run_label(runs: Option<&[StatefulRun]>, started_at_seconds: Option<i64>) -> RunLabel {
    let (Some(runs), Some(started)) = (runs, started_at_seconds) else {
        return RunLabel::Unknown;
    };
    let started_ms = started.saturating_mul(1000);
    runs.iter()
        .find(|run| {
            run.created_at_ms <= started_ms.saturating_add(999)
                && (!run.status.is_terminal() || run.updated_at_ms >= started_ms)
        })
        .map_or(RunLabel::NoRun, |run| RunLabel::Bound {
            run_id: run.id.to_string(),
            status: status_name(run.status),
        })
}

fn status_name(status: StatefulRunStatus) -> &'static str {
    match status {
        StatefulRunStatus::Pending => "pending",
        StatefulRunStatus::Running => "running",
        StatefulRunStatus::Paused => "paused",
        StatefulRunStatus::Completed => "completed",
        StatefulRunStatus::Cancelled => "cancelled",
        StatefulRunStatus::Blocked => "blocked",
        StatefulRunStatus::Failed => "failed",
    }
}

fn captured_turn(
    thread: &StoredThread,
    current_thread: bool,
    runs: Option<&[StatefulRun]>,
    turn: StoredTurn,
) -> Option<CapturedTurn> {
    let (user, answer) = turn_texts(&turn.items);
    if user.is_none() && answer.is_none() {
        return None;
    }
    Some(CapturedTurn {
        thread_id: thread.thread_id.to_string(),
        thread_title: thread
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        current_thread,
        at_ms: turn_time_ms(&turn),
        unfinished_status: turn_status(&turn),
        run: run_label(runs, turn.started_at),
        turn_id: turn.turn_id,
        user,
        answer,
    })
}
