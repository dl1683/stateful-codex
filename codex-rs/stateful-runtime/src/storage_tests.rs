use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::AutonomousClaimOutcome;
use crate::AutonomousClaimRequest;
use crate::NewObligation;
use crate::NewStatefulRun;
use crate::NewSteeringInstruction;
use crate::ObligationPacket;
use crate::RunBudget;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunUpdate;
use crate::SteeringId;
use crate::WorkflowMode;

use super::StatefulRunStore;
use super::StatefulRunStoreError;

#[tokio::test]
async fn run_and_obligation_state_survive_reopen_with_guarded_transitions() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let id = StatefulRunId::parse("run-1").expect("valid run ID");
    let created = store
        .create_run(
            id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Determine the decisive implementation strategy.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run inserts");
    assert_eq!(created.status, StatefulRunStatus::Running);
    let obligation = store
        .append_obligation(
            "obligation-1".to_string(),
            NewObligation {
                project_id: "project-1".to_string(),
                run_id: id.clone(),
                packet: ObligationPacket {
                    learning: vec!["The source contains one decisive constraint.".to_string()],
                    implication: vec!["The implementation strategy must change.".to_string()],
                    next: vec!["Verify the constraint against the exact source.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "turn-1".to_string(),
            },
        )
        .await
        .expect("obligation inserts");
    assert_eq!(obligation.sequence, 1);
    let paused = store
        .update_run(
            &id,
            StatefulRunUpdate {
                expected_revision: created.revision,
                status: StatefulRunStatus::Paused,
                strategy: Some("Verify the decisive constraint first.".to_string()),
                result: None,
            },
        )
        .await
        .expect("run pauses");
    assert_eq!(paused.strategy_revision, 1);

    drop(store);
    let reopened = StatefulRunStore::open(&sqlite)
        .await
        .expect("store reopens");
    assert_eq!(
        reopened.get_run(&id).await.expect("run loads"),
        Some(paused.clone())
    );
    assert_eq!(
        reopened
            .latest_obligation(&id)
            .await
            .expect("obligation loads"),
        Some(obligation.clone())
    );
    assert_eq!(
        reopened
            .run_for_thread("thread-1")
            .await
            .expect("thread run loads")
            .map(|run| run.id),
        Some(id.clone())
    );

    let steering_id = SteeringId::parse("steering-1").expect("valid steering ID");
    let submitted = reopened
        .submit_steering(
            steering_id.clone(),
            NewSteeringInstruction {
                project_id: "project-1".to_string(),
                run_id: id.clone(),
                input: "Connect this constraint to the deployment finding.".to_string(),
                affected_obligation_ids: vec!["obligation-1".to_string()],
            },
        )
        .await
        .expect("steering submits");
    let (resumed, applied) = reopened
        .apply_steering(
            &steering_id,
            crate::SteeringApplication {
                expected_steering_revision: submitted.revision,
                expected_run_revision: paused.revision,
                strategy: "Connect the constraint to the deployment finding.".to_string(),
            },
        )
        .await
        .expect("submitted steering applies atomically");
    assert_eq!(
        applied.resulting_strategy_revision,
        Some(resumed.strategy_revision)
    );
    assert_eq!(
        reopened
            .list_obligations(&id, /*after_sequence*/ None, /*max_results*/ 10)
            .await
            .expect("obligations list"),
        vec![obligation]
    );
    assert_eq!(
        reopened
            .list_steering(&id, /*after*/ None, /*max_results*/ 10)
            .await
            .expect("steering lists"),
        vec![applied]
    );

    let autonomous_id = StatefulRunId::parse("run-auto").expect("valid run ID");
    reopened
        .create_run(
            autonomous_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-auto".to_string()],
                goal: "Complete useful work without supervision.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("autonomous run inserts");
    let claim = AutonomousClaimRequest {
        owner_id: "process-1".to_string(),
        previous_turn_id: "turn-auto-1".to_string(),
        lease_duration_ms: 120_000,
    };
    let claimed = reopened
        .claim_autonomous_continuation(&autonomous_id, claim.clone())
        .await
        .expect("continuation claims");
    assert!(matches!(
        claimed,
        AutonomousClaimOutcome::Claimed {
            run: StatefulRun {
                continuations_used: 1,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        reopened
            .claim_autonomous_continuation(&autonomous_id, claim)
            .await
            .expect("duplicate claim checks"),
        AutonomousClaimOutcome::AlreadyClaimed
    );
    let exhausted = reopened
        .claim_autonomous_continuation(
            &autonomous_id,
            AutonomousClaimRequest {
                owner_id: "process-1".to_string(),
                previous_turn_id: "turn-auto-2".to_string(),
                lease_duration_ms: 120_000,
            },
        )
        .await
        .expect("budget checks");
    assert!(matches!(
        exhausted,
        AutonomousClaimOutcome::BudgetExhausted(StatefulRun {
            status: StatefulRunStatus::Blocked,
            ..
        })
    ));

    let recovery_id = StatefulRunId::parse("run-recovery").expect("valid recovery ID");
    reopened
        .create_run(
            recovery_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-recovery".to_string()],
                goal: "Recover work claimed immediately before a crash.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("recovery run inserts");
    reopened
        .claim_autonomous_continuation(
            &recovery_id,
            AutonomousClaimRequest {
                owner_id: "crashed-process".to_string(),
                previous_turn_id: "turn-before-crash".to_string(),
                lease_duration_ms: 1,
            },
        )
        .await
        .expect("short recovery lease claims");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let recovered = reopened
        .claim_autonomous_continuation(
            &recovery_id,
            AutonomousClaimRequest {
                owner_id: "replacement-process".to_string(),
                previous_turn_id: "turn-before-crash".to_string(),
                lease_duration_ms: 120_000,
            },
        )
        .await
        .expect("expired unstarted continuation reclaims");
    assert!(matches!(
        recovered,
        AutonomousClaimOutcome::Claimed {
            run: StatefulRun {
                continuations_used: 1,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        reopened
            .autonomous_recovery_state(&recovery_id)
            .await
            .expect("recovery state reads")
            .previous_turn_id
            .as_deref(),
        Some("turn-before-crash")
    );
}

#[tokio::test]
async fn terminal_run_rejects_new_steering() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let run_id = StatefulRunId::parse("completed-run").expect("valid run ID");
    let created = store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Complete the bounded investigation.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run inserts");
    store
        .update_run(
            &run_id,
            StatefulRunUpdate {
                expected_revision: created.revision,
                status: StatefulRunStatus::Completed,
                strategy: None,
                result: Some("The investigation is complete.".to_string()),
            },
        )
        .await
        .expect("run completes");

    let error = store
        .submit_steering(
            SteeringId::parse("late-steering").expect("valid steering ID"),
            NewSteeringInstruction {
                project_id: "project-1".to_string(),
                run_id,
                input: "Change the completed strategy.".to_string(),
                affected_obligation_ids: Vec::new(),
            },
        )
        .await
        .expect_err("terminal run rejects steering");

    assert_eq!(
        error.to_string(),
        StatefulRunStoreError::SteeringRunTerminal(StatefulRunStatus::Completed).to_string()
    );
}
