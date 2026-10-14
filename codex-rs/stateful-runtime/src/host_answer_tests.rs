use std::borrow::Cow;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use sqlx::migrate::Migrator;
use tempfile::TempDir;

use super::*;
use crate::AcceptanceChange;
use crate::AcceptanceKind;
use crate::CriterionTerms;
use crate::NewObligation;
use crate::NewStatefulRun;
use crate::NewSteeringInstruction;
use crate::ObligationPacket;
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
        trailer_issues: Vec::new(),
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

    let AnsweredRunOutcome::Answered(answered, committed) = end(&store, &commit).await else {
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
        AnsweredRunOutcome::Answered(answered.clone(), committed.clone())
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
    assert_eq!(record, *committed);
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
    let block_bytes = admitted_answer_bytes(TURN, BLOCK, HostAnswerOutcome::Answered)
        - admitted_answer_bytes(TURN, "", HostAnswerOutcome::Answered);

    for (name, unit, encoded) in [("quotes", "\"", 2), ("multibyte", "é", 2)] {
        let room = MAX_RESULT_BYTES
            - admitted_answer_bytes(TURN, "", HostAnswerOutcome::Answered)
            - block_bytes;
        let fitting = format!(
            "{}{}",
            "a".repeat(room % encoded),
            unit.repeat(room / encoded)
        );
        let run = new_run(&store, &format!("fit-{name}"), WorkflowMode::Autonomous).await;
        let commit = commit_for(&run, &fitting);
        assert_eq!(
            admitted_answer_bytes(TURN, &commit.answer, HostAnswerOutcome::Answered),
            MAX_RESULT_BYTES,
            "{name}"
        );
        assert!(matches!(
            end(&store, &commit).await,
            AnsweredRunOutcome::Answered(..)
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
        let bytes = admitted_answer_bytes(TURN, &commit.answer, HostAnswerOutcome::Answered);
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

fn partial_commit(run: &StatefulRun, result: &str, issues: Vec<String>) -> AnsweredRunCommit {
    AnsweredRunCommit {
        trailer_issues: issues,
        ..commit_for(run, result)
    }
}

/// A11 (i): sixteen 1,024-byte items before a 20,480-byte body keep exactly the items that fit
/// beside the reserved marker; the rest are counted by the marker, and the run reads back
/// partial with the frozen basis.
#[tokio::test]
async fn a_partial_result_keeps_the_items_that_fit_and_counts_the_rest() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let items: Vec<String> = (0..16)
        .map(|index| format!("{index:02}{}", "i".repeat(1_022)))
        .collect();
    let body = "b".repeat(20_480);

    let kept = (MAX_RESULT_BYTES
        - PARTIAL_RESULT_HEADER.len()
        - partial_marker(16).len()
        - 2
        - body.len())
        / 1_027;
    assert_eq!((kept, 16 - kept), (11, 5));
    let expected = format!(
        "{PARTIAL_RESULT_HEADER}{}{}\n{body}",
        items[..kept]
            .iter()
            .map(|item| format!("- {item}\n"))
            .collect::<String>(),
        partial_marker(16 - kept)
    );

    let commit = partial_commit(&run, &body, items);
    let AnsweredRunOutcome::Answered(answered, record) = end(&store, &commit).await else {
        panic!("expected a partial run");
    };
    assert_eq!(answered.status, StatefulRunStatus::Answered);
    assert_eq!(answered.result.as_deref(), Some(expected.as_str()));
    assert_eq!(record.basis, PARTIAL_UNVERIFIED_BASIS);
    assert_eq!(record.outcome(), HostAnswerOutcome::Partial);
    assert_eq!(
        store.host_answer(&run.id).await.expect("read"),
        Some(*record)
    );
}

/// A11 (ii)-(iii): with at least one item, a body that fits beside the header but not beside
/// the reserved marker is refused; when nothing is omitted no marker is written.
#[test]
fn the_marker_is_reserved_and_written_only_on_omission() {
    let items = vec!["x".to_string()];
    let body = "b"
        .repeat(MAX_RESULT_BYTES - PARTIAL_RESULT_HEADER.len() - 2 - partial_marker(1).len() + 1);
    assert!(PARTIAL_RESULT_HEADER.len() + body.len() <= MAX_RESULT_BYTES);
    assert_eq!(compose_partial(&items, &body), None);
    assert!(compose_partial(&items, &body[1..]).is_some());

    assert_eq!(
        compose_partial(&["a".to_string(), "b".to_string()], "The answer."),
        Some(format!("{PARTIAL_RESULT_HEADER}- a\n- b\n\nThe answer."))
    );
}

/// A12: a partial commit whose authorization is refused leaves the run Running with no
/// answer record.
#[tokio::test]
async fn a_refused_partial_commit_writes_nothing() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let run = new_run(&store, "run-1", WorkflowMode::Autonomous).await;
    let deny = || false;
    assert_eq!(
        store
            .end_run_answered(
                &partial_commit(&run, "An answer.", vec!["tests remain".to_string()]),
                &deny
            )
            .await
            .expect("evaluated"),
        AnsweredRunOutcome::NotAuthorized
    );
    assert_eq!(
        store.get_run(&run.id).await.expect("run"),
        Some(run.clone())
    );
    assert_eq!(store.host_answer(&run.id).await.expect("read"), None);
}

/// C2 (L5): Active required criteria and latest blockers are listed; a Derived criterion
/// retired before the final, an optional one and an untouched omission proposal are not, so
/// a run whose only criterion was retired has nothing to list and ends answered.
#[tokio::test]
async fn retired_optional_and_proposed_criteria_are_not_listed() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;
    let derived = |statement: &str, required: bool| AcceptanceChange::Add {
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Manual,
        statement: statement.to_string(),
        request_span: None,
        terms: CriterionTerms {
            required,
            ..CriterionTerms::default()
        },
    };
    let retire = |ordinal: u32| AcceptanceChange::Retire {
        ordinal,
        reason: "Not needed.".to_string(),
    };

    let listed = new_run(&store, "listed", WorkflowMode::Autonomous).await;
    let long = "é".repeat(70);
    let ledger = store
        .revise_acceptance(
            &listed.id,
            /*expected_revision*/ 0,
            vec![
                derived("The parser handles tabs.", true),
                derived("Retire me.", true),
                derived("Nice to have.", false),
                derived(&long, true),
            ],
            "call-add",
            /*project_root*/ None,
        )
        .await
        .expect("criteria added");
    store
        .revise_acceptance(
            &listed.id,
            ledger.revision,
            vec![retire(2)],
            "call-retire",
            /*project_root*/ None,
        )
        .await
        .expect("retired");
    store.propose_uncovered(&listed.id).await.expect("proposed");
    store
        .append_obligation(
            "obligation-1".to_string(),
            NewObligation {
                project_id: "project".to_string(),
                run_id: listed.id.clone(),
                packet: ObligationPacket {
                    blockers: vec!["CI is red.".to_string()],
                    ..ObligationPacket::default()
                },
                provenance_source_id: "call-1".to_string(),
            },
        )
        .await
        .expect("obligation");
    let mut connection = store.pool.acquire().await.expect("connection");
    assert_eq!(
        unverified_items(&mut connection, &listed.id)
            .await
            .expect("items"),
        vec![
            "C1 (The parser handles tabs.): not certified by this terminal answer path".to_string(),
            format!(
                "C4 ({}): not certified by this terminal answer path",
                "é".repeat(60)
            ),
            "CI is red.".to_string(),
        ]
    );

    let retired = new_run(&store, "retired", WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &retired.id,
            /*expected_revision*/ 0,
            vec![derived("Retire me.", true)],
            "call-add",
            /*project_root*/ None,
        )
        .await
        .expect("criterion added");
    store
        .revise_acceptance(
            &retired.id,
            ledger.revision,
            vec![retire(1)],
            "call-retire",
            /*project_root*/ None,
        )
        .await
        .expect("retired");
    assert_eq!(
        unverified_items(&mut connection, &retired.id)
            .await
            .expect("items"),
        Vec::<String>::new()
    );
    drop(connection);
    let AnsweredRunOutcome::Answered(_, record) =
        end(&store, &commit_for(&retired, "An answer.")).await
    else {
        panic!("expected an answered run");
    };
    assert_eq!(record.outcome(), HostAnswerOutcome::Answered);
}

/// The live admission bound is the one in force before the outcome existed. Anchored to
/// that limit, not to the outcome-carrying measurement: a 32,438-byte body behind the test
/// footer is exactly 32,768 serialized bytes in today's shape and is admitted; one byte more
/// is refused. The outcome-carrying measurement of the same answer is 21 bytes larger.
#[tokio::test]
async fn live_admission_keeps_the_limit_from_before_the_outcome() {
    let home = TempDir::new().expect("tempdir");
    let store = open_store(&home).await;

    let fits = new_run(&store, "fits", WorkflowMode::Autonomous).await;
    let commit = commit_for(&fits, &"a".repeat(32_438));
    assert_eq!(
        serialized_answer_bytes(TURN, &commit.answer, HostAnswerOutcome::Answered),
        32_768 + 21
    );
    assert!(matches!(
        end(&store, &commit).await,
        AnsweredRunOutcome::Answered(..)
    ));

    let over = new_run(&store, "over", WorkflowMode::Autonomous).await;
    assert_eq!(
        end(&store, &commit_for(&over, &"a".repeat(32_439))).await,
        AnsweredRunOutcome::Refused(
            "the answer takes 32769 bytes once recorded, over the 32768-byte limit".to_string()
        )
    );
}

/// Items shaped like the omission marker (or starting with the escape) are escaped in the
/// durable result, decode back exactly, and never read as omitted items; the escape counts
/// toward the bound, so an item whose raw line would fit can be omitted once encoded.
#[test]
fn marker_shaped_items_are_escaped_and_counted_encoded() {
    let items = vec![
        "+3 more (read the acceptance ledger and obligation)".to_string(),
        "\\already escaped".to_string(),
        "plain".to_string(),
    ];
    let result = compose_partial(&items, "The answer.").expect("fits");
    assert_eq!(
        result,
        format!(
            "{PARTIAL_RESULT_HEADER}- \\+3 more (read the acceptance ledger and obligation)\n- \\\\already escaped\n- plain\n\nThe answer."
        )
    );
    assert_eq!(
        partial_result_items(&result),
        Some(PartialResultItems { items, omitted: 0 })
    );

    // Exactly 1,027 bytes of room: a 1,024-byte plain item fits, a 1,024-byte item starting
    // with `+` needs 1,028 once escaped and is omitted.
    let body = "b".repeat(
        MAX_RESULT_BYTES - 1_027 - PARTIAL_RESULT_HEADER.len() - partial_marker(1).len() - 2,
    );
    let plain = "a".repeat(1_024);
    assert_eq!(
        partial_result_items(&compose_partial(std::slice::from_ref(&plain), &body).expect("fits")),
        Some(PartialResultItems {
            items: vec![plain],
            omitted: 0
        })
    );
    let shaped = format!("+{}", "a".repeat(1_023));
    assert_eq!(
        partial_result_items(&compose_partial(&[shaped], &body).expect("fits")),
        Some(PartialResultItems {
            items: Vec::new(),
            omitted: 1
        })
    );
}
