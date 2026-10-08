use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::AutonomousClaimOutcome;
use crate::AutonomousClaimRequest;
use crate::NewObligation;
use crate::NewStatefulRun;
use crate::NewStatefulTurnMeasurement;
use crate::NewSteeringInstruction;
use crate::ObligationPacket;
use crate::RunBudget;
use crate::StatefulAttributionCounters;
use crate::StatefulMeasurementSummary;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunUpdate;
use crate::StatefulTokenUsage;
use crate::StatefulTurnStatus;
use crate::StatefulTurnTerminalMeasurement;
use crate::SteeringId;
use crate::SteeringStatus;
use crate::SteeringUpdate;
use crate::TurnTrajectory;
use crate::WorkflowMode;

use super::StatefulRunStore;
use super::StatefulRunStoreError;

#[tokio::test]
async fn turn_measurement_merges_terminal_trajectory_and_is_project_queryable() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let run_id = StatefulRunId::parse("measurement-run").expect("valid run ID");
    store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["measurement-thread".to_string()],
                goal: "Measure the Stateful work.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await
        .expect("run inserts");
    let attribution = NewStatefulTurnMeasurement {
        run_id: run_id.clone(),
        project_id: "project-1".to_string(),
        thread_id: "measurement-thread".to_string(),
        turn_id: "measurement-turn".to_string(),
        status: StatefulTurnStatus::Completed,
        duration_ms: 250,
        attribution_counters: StatefulAttributionCounters {
            root_entries_loaded: 3,
            material_findings_reused: 2,
            ..Default::default()
        },
    };
    let initial = store
        .record_turn_attribution(attribution.clone())
        .await
        .expect("attribution persists");
    assert_eq!(initial.value, attribution);
    assert_eq!(initial.trajectory, None);
    assert_eq!(initial.token_usage, None);
    assert_eq!(initial.completed_at_ms, None);
    assert_eq!(
        store
            .record_turn_attribution(attribution.clone())
            .await
            .expect("identical attribution retry is idempotent"),
        initial
    );

    let trajectory = TurnTrajectory {
        completed_model_responses: 2,
        model_tool_calls: 4,
        tool_output_bytes: 512,
        ..Default::default()
    };
    let token_usage = StatefulTokenUsage {
        total_tokens: 900,
        input_tokens: 700,
        cached_input_tokens: 500,
        cache_write_input_tokens: 25,
        output_tokens: 200,
        reasoning_output_tokens: 75,
    };
    let completed = store
        .record_turn_terminal(
            &run_id,
            "measurement-turn",
            StatefulTurnTerminalMeasurement {
                status: StatefulTurnStatus::Failed,
                completed_at_ms: Some(123_000),
                trajectory: trajectory.clone(),
                token_usage: Some(token_usage.clone()),
            },
        )
        .await
        .expect("trajectory persists");
    assert_eq!(completed.trajectory, Some(trajectory));
    assert_eq!(completed.token_usage, Some(token_usage));
    assert_eq!(completed.value.status, StatefulTurnStatus::Failed);
    assert_eq!(completed.completed_at_ms, Some(123_000));
    let mut conflicting = attribution;
    conflicting.duration_ms += 1;
    assert!(matches!(
        store.record_turn_attribution(conflicting).await,
        Err(StatefulRunStoreError::MeasurementIdentityConflict)
    ));
    assert_eq!(
        store
            .recent_project_measurements("project-1", /*max_results*/ 5)
            .await
            .expect("project measurements load"),
        vec![completed.clone()]
    );
    assert_eq!(
        store
            .summarize_project_measurements("project-1", /*max_results*/ 5)
            .await
            .expect("project measurement summary loads"),
        StatefulMeasurementSummary {
            project_id: "project-1".to_string(),
            measurement_count: 1,
            run_count: 1,
            terminal_measurement_count: 1,
            completed_turns: 0,
            failed_turns: 1,
            aborted_turns: 0,
            turns_with_token_usage: 1,
            duration_ms: 250,
            attribution_counters: StatefulAttributionCounters {
                root_entries_loaded: 3,
                material_findings_reused: 2,
                ..Default::default()
            },
            trajectory: completed.trajectory.clone(),
            token_usage: completed.token_usage.clone(),
            oldest_created_at_ms: Some(completed.created_at_ms),
            newest_created_at_ms: Some(completed.created_at_ms),
            has_more: false,
        }
    );

    for turn_id in ["measurement-turn-2", "measurement-turn-3"] {
        store
            .record_turn_attribution(NewStatefulTurnMeasurement {
                run_id: run_id.clone(),
                project_id: "project-1".to_string(),
                thread_id: "measurement-thread".to_string(),
                turn_id: turn_id.to_string(),
                status: StatefulTurnStatus::Completed,
                duration_ms: 100,
                attribution_counters: StatefulAttributionCounters::default(),
            })
            .await
            .expect("additional measurement persists");
    }
    sqlx::query("UPDATE stateful_turn_measurements SET created_at_ms = 42 WHERE project_id = ?")
        .bind("project-1")
        .execute(&store.pool)
        .await
        .expect("fixture measurements share one ordering timestamp");
    let expected = store
        .recent_project_measurements("project-1", /*max_results*/ 5)
        .await
        .expect("ordered project measurements load");
    assert_eq!(
        store
            .summarize_project_measurements("project-1", /*max_results*/ 2)
            .await
            .expect("bounded project measurement summary loads"),
        StatefulMeasurementSummary {
            project_id: "project-1".to_string(),
            measurement_count: 2,
            run_count: 1,
            terminal_measurement_count: 0,
            completed_turns: 0,
            failed_turns: 0,
            aborted_turns: 0,
            turns_with_token_usage: 0,
            duration_ms: 200,
            attribution_counters: StatefulAttributionCounters::default(),
            trajectory: None,
            token_usage: None,
            oldest_created_at_ms: Some(42),
            newest_created_at_ms: Some(42),
            has_more: true,
        }
    );
    let first = store
        .list_project_measurements("project-1", /*after*/ None, /*max_results*/ 1)
        .await
        .expect("first measurement page loads");
    let first_cursor = first
        .next_cursor
        .clone()
        .expect("first page has a continuation cursor");
    let second = store
        .list_project_measurements("project-1", Some(&first_cursor), /*max_results*/ 1)
        .await
        .expect("second measurement page loads");
    let second_cursor = second
        .next_cursor
        .clone()
        .expect("second page has a continuation cursor");
    let third = store
        .list_project_measurements("project-1", Some(&second_cursor), /*max_results*/ 1)
        .await
        .expect("third measurement page loads");
    assert_eq!(third.next_cursor, None);
    assert_eq!([first.data, second.data, third.data].concat(), expected);
    assert!(matches!(
        store
            .list_project_measurements("project-2", Some(&first_cursor), /*max_results*/ 1,)
            .await,
        Err(StatefulRunStoreError::InvalidListCursor)
    ));
}

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
    let abandoned = reopened
        .abandon_autonomous_continuation(&autonomous_id, "process-1", "turn-auto-1")
        .await
        .expect("unused continuation claim is abandoned")
        .expect("matching claim exists");
    assert_eq!(abandoned.continuations_used, 0);
    assert!(matches!(
        reopened
            .claim_autonomous_continuation(
                &autonomous_id,
                AutonomousClaimRequest {
                    owner_id: "process-1".to_string(),
                    previous_turn_id: "turn-auto-1".to_string(),
                    lease_duration_ms: 120_000,
                },
            )
            .await
            .expect("abandoned continuation can be reclaimed"),
        AutonomousClaimOutcome::Claimed {
            run: StatefulRun {
                continuations_used: 1,
                ..
            },
            ..
        }
    ));
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
async fn final_obligation_and_completion_commit_atomically() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let run_id = StatefulRunId::parse("atomic-completion-run").expect("valid run ID");
    let created = store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["atomic-completion-thread".to_string()],
                goal: "Persist the final result and obligation together.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run inserts");
    let existing = store
        .append_obligation(
            "conflicting-final-obligation".to_string(),
            NewObligation {
                project_id: "project-1".to_string(),
                run_id: run_id.clone(),
                packet: ObligationPacket {
                    learning: vec!["An earlier learning remains recorded.".to_string()],
                    next: vec!["Complete the investigation.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "earlier-turn".to_string(),
            },
        )
        .await
        .expect("earlier obligation inserts");
    let completion_update = StatefulRunUpdate {
        expected_revision: created.revision,
        status: StatefulRunStatus::Completed,
        strategy: None,
        result: Some("The investigation is complete.".to_string()),
    };
    let final_obligation = NewObligation {
        project_id: "project-1".to_string(),
        run_id: run_id.clone(),
        packet: ObligationPacket {
            learning: vec!["The final conclusion is source verified.".to_string()],
            implication: vec!["The result is ready to use.".to_string()],
            ..Default::default()
        },
        provenance_source_id: "completion-turn".to_string(),
    };

    let error = complete(
        &store,
        &run_id,
        completion_update.clone(),
        Some((
            "conflicting-final-obligation".to_string(),
            final_obligation.clone(),
        )),
    )
    .await
    .expect_err("obligation conflict rolls completion back");
    assert_eq!(
        error.to_string(),
        StatefulRunStoreError::ObligationIdentityConflict(
            "conflicting-final-obligation".to_string()
        )
        .to_string()
    );
    assert_eq!(
        store.get_run(&run_id).await.expect("run loads"),
        Some(created)
    );
    assert_eq!(
        store
            .latest_obligation(&run_id)
            .await
            .expect("obligation loads"),
        Some(existing)
    );

    let (completed, obligation) = complete(
        &store,
        &run_id,
        completion_update,
        Some((
            "successful-final-obligation".to_string(),
            final_obligation.clone(),
        )),
    )
    .await
    .expect("completion commits");
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    let obligation = obligation.expect("final obligation");
    assert_eq!(obligation.value, final_obligation);
    assert_eq!(
        store.get_run(&run_id).await.expect("completed run loads"),
        Some(completed)
    );
    assert_eq!(
        store
            .latest_obligation(&run_id)
            .await
            .expect("final obligation loads"),
        Some(obligation)
    );
}

#[tokio::test]
async fn completion_rejects_unresolved_steering_without_mutating_the_run() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let run_id = StatefulRunId::parse("steered-completion-run").expect("valid run ID");
    let created = store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["steered-completion-thread".to_string()],
                goal: "Incorporate user steering before completion.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run inserts");
    let steering_id = SteeringId::parse("unresolved-steering").expect("valid steering ID");
    let submitted = store
        .submit_steering(
            steering_id.clone(),
            NewSteeringInstruction {
                project_id: "project-1".to_string(),
                run_id: run_id.clone(),
                input: "Verify the decisive source before finishing.".to_string(),
                affected_obligation_ids: Vec::new(),
            },
        )
        .await
        .expect("steering inserts");
    let completion_update = StatefulRunUpdate {
        expected_revision: created.revision,
        status: StatefulRunStatus::Completed,
        strategy: None,
        result: Some("The investigation is complete.".to_string()),
    };
    let final_obligation = NewObligation {
        project_id: "project-1".to_string(),
        run_id: run_id.clone(),
        packet: ObligationPacket {
            learning: vec!["The final conclusion is source verified.".to_string()],
            ..Default::default()
        },
        provenance_source_id: "completion-turn".to_string(),
    };

    for (status, status_name) in [
        (SteeringStatus::Submitted, "submitted"),
        (SteeringStatus::Acknowledged, "acknowledged"),
    ] {
        let error = complete(
            &store,
            &run_id,
            completion_update.clone(),
            Some((
                "guarded-final-obligation".to_string(),
                final_obligation.clone(),
            )),
        )
        .await
        .expect_err("unresolved steering rejects completion");
        assert_eq!(
            error.to_string(),
            StatefulRunStoreError::UnresolvedSteering {
                steering_id: steering_id.to_string(),
                status: status_name.to_string(),
            }
            .to_string()
        );
        assert_eq!(
            store.get_run(&run_id).await.expect("run loads"),
            Some(created.clone())
        );
        assert_eq!(
            store
                .latest_obligation(&run_id)
                .await
                .expect("obligation query succeeds"),
            None
        );
        if status == SteeringStatus::Submitted {
            store
                .update_steering(
                    &steering_id,
                    SteeringUpdate {
                        expected_revision: submitted.revision,
                        status: SteeringStatus::Acknowledged,
                        resulting_strategy_revision: None,
                        reason: None,
                    },
                )
                .await
                .expect("steering acknowledges");
        }
    }

    store
        .update_steering(
            &steering_id,
            SteeringUpdate {
                expected_revision: submitted.revision + 1,
                status: SteeringStatus::Rejected,
                resulting_strategy_revision: None,
                reason: Some("The source was already verified exactly.".to_string()),
            },
        )
        .await
        .expect("steering resolves");
    let (completed, obligation) = complete(
        &store,
        &run_id,
        completion_update,
        Some((
            "guarded-final-obligation".to_string(),
            final_obligation.clone(),
        )),
    )
    .await
    .expect("resolved steering permits completion");
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    let obligation = obligation.expect("final obligation");
    assert_eq!(obligation.value, final_obligation);
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
    complete(
        &store,
        &run_id,
        StatefulRunUpdate {
            expected_revision: created.revision,
            status: StatefulRunStatus::Completed,
            strategy: None,
            result: Some("The investigation is complete.".to_string()),
        },
        None,
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

async fn complete(
    store: &StatefulRunStore,
    run_id: &StatefulRunId,
    update: StatefulRunUpdate,
    obligation: Option<(String, NewObligation)>,
) -> Result<(crate::StatefulRun, Option<crate::StatefulObligation>), StatefulRunStoreError> {
    let attempt = store
        .begin_verification(run_id, "test-owner", 60_000)
        .await?;
    let ledger = store.acceptance_ledger(run_id).await?;
    let latest = store.latest_obligation(run_id).await?;
    store
        .complete_run_with_acceptance(
            run_id,
            update,
            &crate::AcceptanceCommit {
                ledger_revision: ledger.revision,
                workspace_generation: ledger.workspace_generation,
                artifacts: std::collections::BTreeMap::new(),
                verification: crate::VerificationClaim {
                    owner: "test-owner".to_string(),
                    attempt,
                },
                validated_obligation_sequence: latest.map(|obligation| obligation.sequence),
            },
            obligation,
        )
        .await
}
