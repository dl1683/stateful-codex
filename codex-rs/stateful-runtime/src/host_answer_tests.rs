use std::borrow::Cow;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::*;
use crate::AutonomousClaimRequest;
use crate::NewObligation;
use crate::NewStatefulRun;
use crate::NewSteeringInstruction;
use crate::ObligationPacket;
use crate::RunBudget;
use crate::StatefulRunModeUpdate;
use crate::StatefulRunUpdate;
use crate::SteeringId;
use crate::SteeringStatus;
use crate::SteeringUpdate;

const THREAD: &str = "thread-1";
const TURN: &str = "turn-1";

async fn open_store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens")
}

async fn new_run(store: &StatefulRunStore, id: &str, mode: WorkflowMode) -> StatefulRun {
    store
        .create_run(
            StatefulRunId::parse(id).expect("run id"),
            NewStatefulRun {
                project_id: "project".to_string(),
                thread_ids: vec![THREAD.to_string()],
                goal: "What does parse_config return?".to_string(),
                mode,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await
        .expect("run created")
}

fn commit_for(run: &StatefulRun, answer: &str) -> HostAnswerCommit {
    HostAnswerCommit {
        run_id: run.id.clone(),
        expected_revision: run.revision,
        thread_id: THREAD.to_string(),
        turn_id: TURN.to_string(),
        answer: answer.to_string(),
    }
}

fn allow() -> bool {
    true
}

async fn refused(store: &StatefulRunStore, commit: &HostAnswerCommit) -> String {
    match store
        .complete_run_with_host_answer(commit, &allow)
        .await
        .expect("host answer evaluated")
    {
        HostAnswerOutcome::Refused(reason) => reason,
        outcome => panic!("expected a refusal, got {outcome:?}"),
    }
}

/// The positive path: the exact answer and the user-judged basis become durable with the
/// Completed status, once, and survive a restart.
#[tokio::test]
async fn host_answer_commits_exact_answer_once_and_survives_restart() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let answer = "parse_config returns Result<Config, Error>.\n";
    let commit = commit_for(&run, answer);

    let HostAnswerOutcome::Committed(completed) = store
        .complete_run_with_host_answer(&commit, &allow)
        .await
        .expect("commit")
    else {
        panic!("expected a commit");
    };
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    assert_eq!(
        completed.result.as_deref(),
        Some("parse_config returns Result<Config, Error>.")
    );
    assert_eq!(completed.revision, run.revision + 1);

    // A retry of the same turn reports the existing commit and changes nothing.
    assert_eq!(
        store
            .complete_run_with_host_answer(&commit, &allow)
            .await
            .expect("retry"),
        HostAnswerOutcome::Committed(completed.clone())
    );
    // Another turn cannot claim it.
    let other = HostAnswerCommit {
        turn_id: "turn-2".to_string(),
        ..commit.clone()
    };
    assert_eq!(
        refused(&store, &other).await,
        "another turn already ended this run with a host answer"
    );

    store.pool.close().await;
    let reopened = open_store(&home).await;
    let record = reopened
        .host_answer(&run.id)
        .await
        .expect("read")
        .expect("record");
    assert_eq!(
        record,
        HostAnswerRecord {
            run_id: run.id.clone(),
            thread_id: THREAD.to_string(),
            turn_id: TURN.to_string(),
            answer: answer.to_string(),
            basis: HOST_ANSWER_BASIS.to_string(),
            committed_at_ms: record.committed_at_ms,
        }
    );
    assert_eq!(
        reopened.get_run(&run.id).await.expect("run"),
        Some(*completed)
    );
}

/// An abort that wins the authorization leaves no trace: the transaction rolls back.
#[tokio::test]
async fn refused_authorization_rolls_back_everything() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let authorized = AtomicBool::new(false);
    let deny = || {
        authorized.store(true, Ordering::SeqCst);
        false
    };
    assert_eq!(
        store
            .complete_run_with_host_answer(&commit_for(&run, "An answer."), &deny)
            .await
            .expect("evaluated"),
        HostAnswerOutcome::NotAuthorized
    );
    assert!(authorized.load(Ordering::SeqCst), "authorization was asked");
    assert_eq!(
        store.get_run(&run.id).await.expect("run"),
        Some(run.clone())
    );
    assert_eq!(store.host_answer(&run.id).await.expect("read"), None);
}

/// T29: every durable fact the finalizer read can change before the terminal transaction;
/// the transaction re-checks each one, and authorization is never asked when one refuses.
#[tokio::test]
async fn terminal_transaction_rechecks_every_durable_precondition() {
    let never = || -> bool { panic!("authorization must not be asked for a refused commit") };
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;

    // Not Autonomous.
    let collaborative = new_run(&store, "collaborative", WorkflowMode::Collaborative).await;
    let outcome = store
        .complete_run_with_host_answer(&commit_for(&collaborative, "An answer."), &never)
        .await
        .expect("evaluated");
    assert_eq!(
        outcome,
        HostAnswerOutcome::Refused("the run is no longer a Running Autonomous run".to_string())
    );

    // Revision moved (any update between the read and the transaction).
    let run = new_run(&store, "revision", WorkflowMode::Autonomous).await;
    let stale = commit_for(&run, "An answer.");
    store
        .update_mode(
            &run.id,
            StatefulRunModeUpdate {
                expected_revision: run.revision,
                mode: WorkflowMode::Collaborative,
            },
        )
        .await
        .expect("mode changed");
    assert_eq!(
        refused(&store, &stale).await,
        format!(
            "the run changed (revision {} instead of {})",
            run.revision + 1,
            run.revision
        )
    );

    // Paused.
    let run = new_run(&store, "paused", WorkflowMode::Autonomous).await;
    let paused = store
        .update_run(
            &run.id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Paused,
                strategy: None,
                result: None,
            },
        )
        .await
        .expect("paused");
    assert_eq!(
        refused(&store, &commit_for(&paused, "An answer.")).await,
        "the run is no longer a Running Autonomous run"
    );

    // Wrong or extra thread binding.
    let run = new_run(&store, "binding", WorkflowMode::Autonomous).await;
    let foreign = HostAnswerCommit {
        thread_id: "thread-2".to_string(),
        ..commit_for(&run, "An answer.")
    };
    assert_eq!(
        refused(&store, &foreign).await,
        "the run is not bound to exactly the answering thread"
    );

    const RECORDED_WORK: &str = "the run already has recorded work (a continuation, obligation, steering, or acceptance record)";

    // A claimed continuation: the run is past its first task.
    let run = new_run(&store, "continued", WorkflowMode::Autonomous).await;
    store
        .claim_autonomous_continuation(
            &run.id,
            AutonomousClaimRequest {
                owner_id: "owner".to_string(),
                previous_turn_id: TURN.to_string(),
                lease_duration_ms: 60_000,
            },
        )
        .await
        .expect("claimed");
    let current = store.get_run(&run.id).await.expect("run").expect("exists");
    assert_eq!(
        refused(&store, &commit_for(&current, "An answer.")).await,
        RECORDED_WORK
    );

    // An obligation, with or without blockers.
    let run = new_run(&store, "obligation", WorkflowMode::Autonomous).await;
    store
        .append_obligation(
            "obligation-1".to_string(),
            NewObligation {
                project_id: "project".to_string(),
                run_id: run.id.clone(),
                packet: ObligationPacket {
                    blockers: vec!["Need the user's key.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "source".to_string(),
            },
        )
        .await
        .expect("obligation");
    assert_eq!(
        refused(&store, &commit_for(&run, "An answer.")).await,
        RECORDED_WORK
    );

    // Steering in every non-rejected state; rejected steering does not block.
    for status in [
        SteeringStatus::Submitted,
        SteeringStatus::Acknowledged,
        SteeringStatus::Rejected,
    ] {
        let run = new_run(
            &store,
            &format!("steering-{status:?}"),
            WorkflowMode::Autonomous,
        )
        .await;
        let steering = store
            .submit_steering(
                SteeringId::parse(format!("steer-{status:?}")).expect("steering id"),
                NewSteeringInstruction {
                    project_id: "project".to_string(),
                    run_id: run.id.clone(),
                    input: "Also cover the error path.".to_string(),
                    affected_obligation_ids: Vec::new(),
                },
            )
            .await
            .expect("steering submitted");
        if status != SteeringStatus::Submitted {
            store
                .update_steering(
                    &steering.id,
                    SteeringUpdate {
                        expected_revision: steering.revision,
                        status,
                        resulting_strategy_revision: None,
                        reason: Some("Handled.".to_string()),
                    },
                )
                .await
                .expect("steering updated");
        }
        let current = store.get_run(&run.id).await.expect("run").expect("exists");
        let outcome = store
            .complete_run_with_host_answer(&commit_for(&current, "An answer."), &allow)
            .await
            .expect("evaluated");
        if status == SteeringStatus::Rejected {
            assert!(
                matches!(outcome, HostAnswerOutcome::Committed(_)),
                "{outcome:?}"
            );
        } else {
            assert_eq!(
                outcome,
                HostAnswerOutcome::Refused(RECORDED_WORK.to_string())
            );
        }
    }

    // A pending command and a workspace generation change.
    let run = new_run(&store, "pending", WorkflowMode::Autonomous).await;
    store.begin_command(&run.id, "call-1").await.expect("begun");
    assert_eq!(
        refused(&store, &commit_for(&run, "An answer.")).await,
        RECORDED_WORK
    );
    let run = new_run(&store, "generation", WorkflowMode::Autonomous).await;
    store
        .bump_workspace_generation(&run.id)
        .await
        .expect("bumped");
    assert_eq!(
        refused(&store, &commit_for(&run, "An answer.")).await,
        RECORDED_WORK
    );
}

/// T33: generic writers still cannot complete; the host answer is its own typed path.
#[tokio::test]
async fn generic_updates_still_need_the_acceptance_decision() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let error = store
        .update_run(
            &run.id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Completed,
                strategy: None,
                result: Some("An answer.".to_string()),
            },
        )
        .await
        .expect_err("generic completion refused");
    assert!(
        matches!(
            error,
            StatefulRunStoreError::CompletionRequiresAcceptanceDecision
        ),
        "{error:?}"
    );
    assert_eq!(store.host_answer(&run.id).await.expect("read"), None);
}

/// T31: the whole answer plus its basis must fit the result budget before anything is
/// written; a fitting multibyte answer is kept byte for byte, one byte more is refused.
#[tokio::test]
async fn answer_and_basis_budget_is_checked_before_any_write() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let room = MAX_RESULT_BYTES - HOST_ANSWER_BASIS.len();
    // "é" is two bytes; pad with one ASCII byte when the room is odd.
    let mut fitting = "é".repeat(room / 2);
    if room % 2 == 1 {
        fitting.push('"');
    }
    assert_eq!(fitting.len(), room);

    let over = format!("{fitting}x");
    let run = new_run(&store, "over", WorkflowMode::Autonomous).await;
    assert_eq!(
        refused(&store, &commit_for(&run, &over)).await,
        format!(
            "the final answer and its basis take {} bytes, over the {MAX_RESULT_BYTES}-byte result limit",
            MAX_RESULT_BYTES + 1
        )
    );
    assert_eq!(
        store.get_run(&run.id).await.expect("run"),
        Some(run.clone())
    );

    let run = new_run(&store, "fits", WorkflowMode::Autonomous).await;
    let outcome = store
        .complete_run_with_host_answer(&commit_for(&run, &fitting), &allow)
        .await
        .expect("evaluated");
    assert!(
        matches!(outcome, HostAnswerOutcome::Committed(_)),
        "{outcome:?}"
    );
    assert_eq!(
        store
            .host_answer(&run.id)
            .await
            .expect("read")
            .expect("record")
            .answer,
        fitting
    );
    assert_eq!(
        refused(
            &store,
            &commit_for(
                &new_run(&store, "blank", WorkflowMode::Autonomous).await,
                " \n"
            )
        )
        .await,
        "the final answer is empty or contains NUL"
    );
}

/// T34: a store that applied 0011/0012 (including an old readOnly exemption row) upgrades
/// additively; old exemption state is never read as host-answer eligibility.
#[tokio::test]
async fn upgrade_from_applied_history_keeps_old_rows_and_meaning() {
    let home = TempDir::new().expect("tempdir");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let prefix = Migrator {
        migrations: Cow::Owned(
            crate::storage::MIGRATOR
                .iter()
                .filter(|migration| migration.version <= 12)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    };
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join(crate::storage::DATABASE_NAME))
        .await
        .expect("pool");
    sqlite
        .run_migrations(&pool, &prefix)
        .await
        .expect("history applied");
    sqlx::query("INSERT INTO stateful_runs (id, project_id, goal, mode, status, strategy_revision, revision, created_at_ms, updated_at_ms) VALUES ('legacy', 'project', 'What is X?', 'autonomous', 'running', 0, 1, 1, 1)")
        .execute(&pool)
        .await
        .expect("legacy run");
    sqlx::query("INSERT INTO stateful_run_threads VALUES ('legacy', 0, 'thread-1')")
        .execute(&pool)
        .await
        .expect("legacy binding");
    sqlx::query("INSERT INTO stateful_acceptance_ledgers (run_id, revision, workspace_generation, observed_executions, stalled_completions, stalled_fingerprint, verification_attempt, updated_at_ms, exemption, observation_version, completion_attempts) VALUES ('legacy', 0, 0, 0, 0, '', 0, 1, 'readOnly', 1, 1)")
        .execute(&pool)
        .await
        .expect("legacy exemption row");
    pool.close().await;

    let store = StatefulRunStore::open(&sqlite).await.expect("upgraded");
    let legacy = store
        .get_run(&StatefulRunId::parse("legacy").expect("id"))
        .await
        .expect("read")
        .expect("legacy run kept");
    assert_eq!(legacy.value.goal, "What is X?");
    assert_eq!(
        refused(&store, &commit_for(&legacy, "An answer.")).await,
        "the run already has recorded work (a continuation, obligation, steering, or acceptance record)"
    );

    let fresh = new_run(&store, "fresh", WorkflowMode::Autonomous).await;
    assert!(matches!(
        store
            .complete_run_with_host_answer(&commit_for(&fresh, "An answer."), &allow)
            .await
            .expect("evaluated"),
        HostAnswerOutcome::Committed(_)
    ));
    store.pool.close().await;
    // Reopening verifies every applied checksum, including 0011 and 0012.
    let reopened = StatefulRunStore::open(&sqlite).await.expect("reopened");
    assert!(
        reopened
            .host_answer(&fresh.id)
            .await
            .expect("read")
            .is_some()
    );
}
