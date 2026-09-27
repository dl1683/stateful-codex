use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::NewStatefulTurnMeasurement;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulAttributionCounters;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulTokenUsage;
use codex_stateful_runtime::StatefulTurnStatus;
use codex_stateful_runtime::StatefulTurnTerminalMeasurement;
use codex_stateful_runtime::TurnTrajectory;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tempfile::TempDir;

use super::MeasurementKey;
use super::StatefulStoreHandle;

#[tokio::test]
async fn terminal_trajectory_merges_when_it_arrives_before_attribution() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let handle = StatefulStoreHandle::new(Some(sqlite));
    let store = handle
        .get()
        .await
        .expect("store opens")
        .expect("test store is configured");
    let run_id = StatefulRunId::parse("terminal-first-run").expect("valid run ID");
    store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Measure a failed turn.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 60,
                },
            },
        )
        .await
        .expect("run inserts");
    let key = MeasurementKey {
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
    };
    handle
        .expected_measurements
        .lock()
        .expect("expected measurement lock")
        .insert(key.clone());
    let trajectory = TurnTrajectory {
        completed_model_responses: 1,
        model_tool_calls: 2,
        ..Default::default()
    };
    let token_usage = StatefulTokenUsage {
        total_tokens: 30,
        input_tokens: 20,
        cached_input_tokens: 10,
        cache_write_input_tokens: 0,
        output_tokens: 10,
        reasoning_output_tokens: 5,
    };

    let terminal_handle = handle.clone();
    let terminal_key = key.clone();
    let terminal_trajectory = trajectory.clone();
    let terminal_task = tokio::spawn(async move {
        terminal_handle
            .persist_terminal(
                &terminal_key,
                StatefulTurnTerminalMeasurement {
                    status: StatefulTurnStatus::Failed,
                    completed_at_ms: Some(2_000),
                    trajectory: terminal_trajectory,
                    token_usage: Some(token_usage),
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if handle
                .measurement_merge
                .lock()
                .expect("measurement merge lock")
                .pending_trajectories
                .contains_key(&key)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("terminal half becomes pending");
    handle
        .persist_attribution(
            &key,
            NewStatefulTurnMeasurement {
                run_id: run_id.clone(),
                project_id: "project-1".to_string(),
                thread_id: key.thread_id.clone(),
                turn_id: key.turn_id.clone(),
                status: StatefulTurnStatus::Completed,
                duration_ms: 50,
                attribution_counters: StatefulAttributionCounters::default(),
            },
        )
        .await
        .expect("attribution completes the merge");
    terminal_task
        .await
        .expect("terminal task joins")
        .expect("terminal task observes the merge");

    let stored = store
        .get_turn_measurement(&run_id, &key.turn_id)
        .await
        .expect("measurement loads")
        .expect("measurement exists");
    assert_eq!(stored.value.status, StatefulTurnStatus::Failed);
    assert_eq!(stored.trajectory, Some(trajectory));
    assert_eq!(
        stored.token_usage,
        Some(StatefulTokenUsage {
            total_tokens: 30,
            input_tokens: 20,
            cached_input_tokens: 10,
            cache_write_input_tokens: 0,
            output_tokens: 10,
            reasoning_output_tokens: 5,
        })
    );
    assert_eq!(stored.completed_at_ms, Some(2_000));
    assert!(
        !handle
            .expected_measurements
            .lock()
            .expect("expected measurement lock")
            .contains(&key)
    );
}
