//! Gathers the continuity record from the thread store's per-turn summaries (first user
//! message and final answer) of the selected project's recent top-level threads.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::TurnRun;
use codex_stateful_runtime::WorkflowMode;
use codex_thread_store::ListTurnsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::StoredTurn;
use codex_thread_store::StoredTurnItemsView;
use codex_thread_store::StoredTurnStatus;
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
/// Recorded turn bindings read per thread; older turns of busier threads get "unknown".
const MAX_TURN_RUNS_PER_THREAD: u32 = 100;

/// Reads the newest turn summaries of the project's top-level, unarchived threads, labelled
/// with the run inferred to own each turn, and the project's latest run.
pub(super) async fn gather_continuity(
    threads: &dyn ThreadStore,
    runtime: Option<&StatefulRunStore>,
    blackboard: Option<&codex_project_intelligence::BlackboardStore>,
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
        let turn_runs = match runtime {
            Some(runtime) => runtime
                .turn_runs_for_thread(&thread_id, MAX_TURN_RUNS_PER_THREAD)
                .await
                .ok(),
            None => None,
        };
        let current_thread = thread_id == current_thread_id;
        for turn in page.turns {
            if let Some(mut captured) =
                captured_turn(thread, current_thread, turn_runs.as_deref(), turn)
            {
                let Some(store) = blackboard else {
                    history_unavailable = true;
                    continue;
                };
                if !store
                    .source_turn_eligible(project_id, &captured.turn_id)
                    .await
                    .unwrap_or(false)
                {
                    history_unavailable = true;
                    continue;
                }
                for text in [
                    &mut captured.user,
                    &mut captured.answer,
                    &mut captured.thread_title,
                ] {
                    if let Some(value) = text.as_ref()
                        && !store
                            .source_text_eligible(project_id, value)
                            .await
                            .unwrap_or(false)
                    {
                        *text = None;
                        history_unavailable = true;
                    }
                }
                if captured.user.is_some() || captured.answer.is_some() {
                    turns.push(captured);
                }
            }
        }
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
        more_turns,
        unreadable_threads,
        history_unavailable,
        latest_run: match (runtime, newest_bound_run(&turns)) {
            (Some(runtime), Some(run_id)) => latest_run(runtime, &run_id).await,
            _ => None,
        },
        turns,
    }
}

/// The run bound to the newest captured turn that recorded one, so the latest-run line
/// follows the same thread selection (unarchived, top-level) as the turns.
fn newest_bound_run(turns: &[CapturedTurn]) -> Option<StatefulRunId> {
    turns.iter().find_map(|turn| match &turn.run {
        RunLabel::Bound { run_id, .. } => StatefulRunId::parse(run_id.clone()).ok(),
        RunLabel::NoRun | RunLabel::Unknown => None,
    })
}

async fn latest_run(runtime: &StatefulRunStore, run_id: &StatefulRunId) -> Option<LatestRun> {
    let run = match runtime.get_run(run_id).await {
        Ok(run) => run?,
        Err(error) => {
            tracing::warn!(%run_id, %error, "failed to load the latest Stateful run");
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

/// The run the host recorded for a finished turn. `turn_runs` is `None` when the recorded
/// bindings could not be read; unfinished turns have no recorded binding yet.
fn run_label(turn_runs: Option<&[TurnRun]>, turn: &StoredTurn) -> RunLabel {
    let Some(turn_runs) = turn_runs else {
        return RunLabel::Unknown;
    };
    if turn.status == StoredTurnStatus::InProgress {
        return RunLabel::Unknown;
    }
    match turn_runs
        .iter()
        .find(|turn_run| turn_run.turn_id == turn.turn_id)
    {
        Some(turn_run) => RunLabel::Bound {
            run_id: turn_run.run_id.to_string(),
            status: status_name(turn_run.run_status),
        },
        None if turn_runs.len() < MAX_TURN_RUNS_PER_THREAD as usize => RunLabel::NoRun,
        None => RunLabel::Unknown,
    }
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
    turn_runs: Option<&[TurnRun]>,
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
        run: run_label(turn_runs, &turn),
        turn_id: turn.turn_id,
        user,
        answer,
    })
}
