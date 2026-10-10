use std::borrow::Cow;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::*;
use crate::NewStatefulRun;
use crate::NewSteeringInstruction;
use crate::RunBudget;
use crate::StatefulRunModeUpdate;
use crate::StatefulRunUpdate;
use crate::SteeringId;

const THREAD: &str = "thread-1";
const TURN: &str = "turn-1";
const BLOCK: &str =
    "\n\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

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

fn commit_for(run: &StatefulRun, result: &str) -> AnsweredRunCommit {
    AnsweredRunCommit {
        run_id: run.id.clone(),
        thread_id: THREAD.to_string(),
        turn_id: TURN.to_string(),
        answer: format!("{result}{BLOCK}"),
        result: result.to_string(),
    }
}

fn allow() -> bool {
    true
}

async fn end(store: &StatefulRunStore, commit: &AnsweredRunCommit) -> AnsweredRunOutcome {
    store
        .end_run_answered(commit, &allow)
        .await
        .expect("answer evaluated")
}

/// The positive path: the run reads back as Answered (never Completed) with the exact answer
/// and the unverified basis, once, survives a restart, and never changes again.
#[tokio::test]
async fn answered_run_keeps_its_exact_answer_and_survives_restart() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let commit = commit_for(&run, "parse_config returns Result<Config, Error>.");

    let AnsweredRunOutcome::Answered(answered) = end(&store, &commit).await else {
        panic!("expected an answered run");
    };
    assert_eq!(answered.status, StatefulRunStatus::Answered);
    assert_eq!(
        answered.result.as_deref(),
        Some("parse_config returns Result<Config, Error>.")
    );
    assert_eq!(answered.revision, run.revision + 1);
    // A retry of the same turn reports the same run; another turn cannot end it again.
    assert_eq!(
        end(&store, &commit).await,
        AnsweredRunOutcome::Answered(answered.clone())
    );
    let other = AnsweredRunCommit {
        turn_id: "turn-2".to_string(),
        ..commit.clone()
    };
    assert_eq!(end(&store, &other).await, AnsweredRunOutcome::NotEligible);
    // Terminal: no thread lookup finds it and no writer changes it.
    assert_eq!(store.run_for_thread(THREAD).await.expect("lookup"), None);
    for status in [StatefulRunStatus::Completed, StatefulRunStatus::Answered] {
        assert!(matches!(
            store
                .update_run(
                    &run.id,
                    StatefulRunUpdate {
                        expected_revision: answered.revision,
                        status,
                        strategy: None,
                        result: Some("done".to_string()),
                    },
                )
                .await,
            Err(StatefulRunStoreError::InvalidTransition { .. })
        ));
    }
    assert!(matches!(
        store
            .update_mode(
                &run.id,
                StatefulRunModeUpdate {
                    expected_revision: answered.revision,
                    mode: WorkflowMode::Collaborative,
                },
            )
            .await,
        Err(StatefulRunStoreError::InvalidModeTransition)
    ));

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
            answer: commit.answer.clone(),
            basis: ANSWERED_UNVERIFIED_BASIS.to_string(),
            committed_at_ms: record.committed_at_ms,
        }
    );
    assert_eq!(
        reopened.get_run(&run.id).await.expect("run"),
        Some(*answered)
    );
}

/// An abort that wins the authorization leaves no trace: the transaction rolls back.
#[tokio::test]
async fn refused_authorization_rolls_back_everything() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let asked = AtomicBool::new(false);
    let deny = || {
        asked.store(true, Ordering::SeqCst);
        false
    };
    assert_eq!(
        store
            .end_run_answered(&commit_for(&run, "An answer."), &deny)
            .await
            .expect("evaluated"),
        AnsweredRunOutcome::NotAuthorized
    );
    assert!(asked.load(Ordering::SeqCst), "authorization was asked");
    assert_eq!(
        store.get_run(&run.id).await.expect("run"),
        Some(run.clone())
    );
    assert_eq!(store.host_answer(&run.id).await.expect("read"), None);
}

/// Only a running Autonomous run bound to the answering thread ends answered, and never
/// while steering waits; nothing is written otherwise.
#[tokio::test]
async fn only_a_running_autonomous_run_of_the_thread_ends_answered() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;

    let collaborative = new_run(&store, "collaborative", WorkflowMode::Collaborative).await;
    assert_eq!(
        end(&store, &commit_for(&collaborative, "An answer.")).await,
        AnsweredRunOutcome::NotEligible
    );

    let foreign = new_run(&store, "foreign", WorkflowMode::Autonomous).await;
    let other_thread = AnsweredRunCommit {
        thread_id: "thread-2".to_string(),
        ..commit_for(&foreign, "An answer.")
    };
    assert_eq!(
        end(&store, &other_thread).await,
        AnsweredRunOutcome::NotEligible
    );

    let paused = new_run(&store, "paused", WorkflowMode::Autonomous).await;
    store
        .update_run(
            &paused.id,
            StatefulRunUpdate {
                expected_revision: paused.revision,
                status: StatefulRunStatus::Paused,
                strategy: None,
                result: None,
            },
        )
        .await
        .expect("paused");
    assert_eq!(
        end(&store, &commit_for(&paused, "An answer.")).await,
        AnsweredRunOutcome::NotEligible
    );

    let steered = new_run(&store, "steered", WorkflowMode::Autonomous).await;
    store
        .submit_steering(
            SteeringId::parse("steer-1").expect("steering id"),
            NewSteeringInstruction {
                project_id: "project".to_string(),
                run_id: steered.id.clone(),
                input: "Also cover the error path.".to_string(),
                affected_obligation_ids: Vec::new(),
            },
        )
        .await
        .expect("steering submitted");
    assert_eq!(
        end(&store, &commit_for(&steered, "An answer.")).await,
        AnsweredRunOutcome::Refused(
            "steering for this run is still waiting to be applied".to_string()
        )
    );

    for run in [collaborative, foreign, steered] {
        assert_eq!(store.host_answer(&run.id).await.expect("read"), None);
        assert_ne!(
            store
                .get_run(&run.id)
                .await
                .expect("run")
                .expect("run")
                .status,
            StatefulRunStatus::Answered
        );
    }
}

/// The bound is measured on the serialized record: escapes count, so an answer whose raw
/// bytes fit can still be refused. An exact fit is kept byte for byte; one byte more, or an
/// empty answer, is refused before anything is written.
#[tokio::test]
async fn the_answer_bound_is_measured_on_the_serialized_record() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let block_bytes = serialized_answer_bytes(TURN, BLOCK) - serialized_answer_bytes(TURN, "");

    for (name, unit, encoded) in [("quotes", "\"", 2), ("multibyte", "é", 2)] {
        let room = MAX_RESULT_BYTES - serialized_answer_bytes(TURN, "") - block_bytes;
        let fitting = format!(
            "{}{}",
            "a".repeat(room % encoded),
            unit.repeat(room / encoded)
        );
        let run = new_run(&store, &format!("fit-{name}"), WorkflowMode::Autonomous).await;
        let commit = commit_for(&run, &fitting);
        assert_eq!(
            serialized_answer_bytes(TURN, &commit.answer),
            MAX_RESULT_BYTES,
            "{name}"
        );
        assert!(matches!(
            end(&store, &commit).await,
            AnsweredRunOutcome::Answered(_)
        ));
        assert_eq!(
            store
                .host_answer(&run.id)
                .await
                .expect("read")
                .expect("record")
                .answer,
            commit.answer
        );

        let over = new_run(&store, &format!("over-{name}"), WorkflowMode::Autonomous).await;
        let commit = commit_for(&over, &format!("{fitting}{unit}"));
        assert!(commit.answer.len() < MAX_RESULT_BYTES || name == "multibyte");
        let bytes = serialized_answer_bytes(TURN, &commit.answer);
        assert_eq!(
            end(&store, &commit).await,
            AnsweredRunOutcome::Refused(format!(
                "the answer takes {bytes} bytes once recorded, over the {MAX_RESULT_BYTES}-byte limit"
            ))
        );
        assert_eq!(store.host_answer(&over.id).await.expect("read"), None);
    }

    let empty = new_run(&store, "empty", WorkflowMode::Autonomous).await;
    assert_eq!(
        end(&store, &commit_for(&empty, "  ")).await,
        AnsweredRunOutcome::Refused("the answer is empty or malformed".to_string())
    );
}

/// A store that applied 0013 with an earlier Completed host answer reads that run as
/// Answered: a recorded host answer is never presented as a verified completion.
#[tokio::test]
async fn an_applied_host_answer_reads_as_answered() {
    let home = TempDir::new().expect("tempdir");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let history = Migrator {
        migrations: Cow::Owned(
            crate::storage::MIGRATOR
                .iter()
                .filter(|migration| migration.version <= 13)
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
        .run_migrations(&pool, &history)
        .await
        .expect("history applied");
    sqlx::query("INSERT INTO stateful_runs (id, project_id, goal, mode, status, strategy_revision, result, revision, created_at_ms, updated_at_ms) VALUES ('legacy', 'project', 'What is X?', 'autonomous', 'completed', 0, 'X is Y.', 2, 1, 1)")
        .execute(&pool)
        .await
        .expect("legacy run");
    sqlx::query("INSERT INTO stateful_run_threads VALUES ('legacy', 0, 'thread-1')")
        .execute(&pool)
        .await
        .expect("legacy binding");
    sqlx::query("INSERT INTO stateful_host_answers VALUES ('legacy', 'thread-1', 'turn-1', 'X is Y.', 'Host-ended answer.', 1)")
        .execute(&pool)
        .await
        .expect("legacy answer");
    pool.close().await;

    let store = StatefulRunStore::open(&sqlite).await.expect("opened");
    let legacy = store
        .get_run(&StatefulRunId::parse("legacy").expect("id"))
        .await
        .expect("read")
        .expect("legacy run kept");
    assert_eq!(legacy.status, StatefulRunStatus::Answered);
    assert_eq!(legacy.result.as_deref(), Some("X is Y."));
}
