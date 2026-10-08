use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::NewStatefulRun;
use crate::NewStatefulTurnMeasurement;
use crate::RunBudget;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunUpdate;
use crate::StatefulTurnStatus;
use crate::TurnRun;
use crate::WorkflowMode;

async fn run_with_turn(
    store: &StatefulRunStore,
    run: &str,
    thread: &str,
    turn: &str,
) -> StatefulRunId {
    let run_id = StatefulRunId::parse(run).expect("valid run ID");
    store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec![thread.to_string()],
                goal: "Continue the work.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await
        .expect("run inserts");
    store
        .record_turn_attribution(NewStatefulTurnMeasurement {
            run_id: run_id.clone(),
            project_id: "project-1".to_string(),
            thread_id: thread.to_string(),
            turn_id: turn.to_string(),
            status: StatefulTurnStatus::Completed,
            duration_ms: 10,
            attribution_counters: Default::default(),
        })
        .await
        .expect("attribution persists");
    run_id
}

#[tokio::test]
async fn turn_runs_report_the_recorded_run_of_each_finished_turn() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(temp_dir.path().abs()))
        .await
        .expect("store opens");
    let first = run_with_turn(&store, "run-first", "thread-1", "turn-1").await;
    let first_run = store
        .get_run(&first)
        .await
        .expect("run loads")
        .expect("run exists");
    let attempt = store
        .begin_verification(&first, "test-owner", 60_000)
        .await
        .expect("verification starts");
    let ledger = store.acceptance_ledger(&first).await.expect("ledger reads");
    store
        .complete_run_with_acceptance(
            &first,
            StatefulRunUpdate {
                expected_revision: first_run.revision,
                status: StatefulRunStatus::Completed,
                strategy: None,
                result: Some("Done.".to_string()),
            },
            &crate::AcceptanceCommit {
                ledger_revision: ledger.revision,
                workspace_generation: ledger.workspace_generation,
                artifacts: std::collections::BTreeMap::new(),
                verification: crate::VerificationClaim {
                    owner: "test-owner".to_string(),
                    attempt,
                },
                validated_obligation_sequence: None,
            },
            /*obligation*/ None,
        )
        .await
        .expect("first run completes");
    let second = run_with_turn(&store, "run-second", "thread-1", "turn-2").await;
    run_with_turn(&store, "run-other", "thread-2", "turn-3").await;

    let mut turn_runs = store
        .turn_runs_for_thread("thread-1", /*max_results*/ 20)
        .await
        .expect("turn runs load");
    turn_runs.sort_by(|left, right| left.turn_id.cmp(&right.turn_id));
    assert_eq!(
        turn_runs,
        vec![
            TurnRun {
                turn_id: "turn-1".to_string(),
                run_id: first,
                run_status: StatefulRunStatus::Completed,
            },
            TurnRun {
                turn_id: "turn-2".to_string(),
                run_id: second,
                run_status: StatefulRunStatus::Running,
            },
        ]
    );
}
