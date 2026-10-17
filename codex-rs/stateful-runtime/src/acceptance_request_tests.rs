use super::acceptance_request;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::NewStatefulRun;
use crate::NewSteeringInstruction;
use crate::RunBudget;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunUpdate;
use crate::SteeringApplication;
use crate::SteeringId;
use crate::SteeringStatus;
use crate::SteeringUpdate;
use crate::WorkflowMode;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn fixture(
    goal: &str,
) -> TestResult<(TempDir, SqliteConfig, StatefulRunStore, StatefulRunId)> {
    let home = tempfile::tempdir()?;
    let sqlite = SqliteConfig::from_sqlite_home(home.path().abs());
    let store = StatefulRunStore::open(&sqlite).await?;
    let id = StatefulRunId::parse("request")?;
    store
        .create_run(
            id.clone(),
            NewStatefulRun {
                project_id: "legacy-project".into(),
                thread_ids: vec!["thread".into()],
                goal: goal.into(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await?;
    Ok((home, sqlite, store, id))
}

#[tokio::test]
async fn explicit_goal_preserves_whole_request_and_stable_utf8_parts_after_reopen() -> TestResult {
    let goal = "Keep ₹17.25 and the \"A\" caveat.";
    let (_home, sqlite, store, id) = fixture(goal).await?;
    let inputs = [("s2", "पहिली सूचना."), ("s1", "Retain the second caveat.")];
    let mut run = store.get_run(&id).await?.ok_or("run")?;
    for (index, (name, input)) in inputs.iter().enumerate() {
        let steering_id = SteeringId::parse(*name)?;
        let steering = store
            .submit_steering(
                steering_id.clone(),
                NewSteeringInstruction {
                    project_id: run.value.project_id.clone(),
                    run_id: id.clone(),
                    input: (*input).into(),
                    affected_obligation_ids: Vec::new(),
                },
            )
            .await?;
        (run, _) = store
            .apply_steering(
                &steering_id,
                SteeringApplication {
                    expected_steering_revision: steering.revision,
                    expected_run_revision: run.revision,
                    strategy: format!("Initial strategy {index}"),
                },
            )
            .await?;
    }
    let expected = format!("{goal}\n{}\n{}", inputs[0].1, inputs[1].1);
    let first = goal.len() + 1;
    let second = first + inputs[0].1.len() + 1;
    let expected_parts: Vec<(String, std::ops::Range<usize>)> = vec![
        ("s2".into(), first..second - 1),
        ("s1".into(), second..expected.len()),
    ];
    let mut transaction = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    let actual = acceptance_request(&mut transaction, &id, goal).await?;
    transaction.commit().await?;
    assert_eq!(
        (actual.text, actual.parts),
        (expected.clone(), expected_parts.clone())
    );
    assert_eq!(store.acceptance_request(&id).await?, expected);
    store.pool.close().await;

    let store = StatefulRunStore::open(&sqlite).await?;
    let steering_id = SteeringId::parse("s0")?;
    let input = "Later, preserve 83.50.";
    let steering = store
        .submit_steering(
            steering_id.clone(),
            NewSteeringInstruction {
                project_id: run.value.project_id.clone(),
                run_id: id.clone(),
                input: input.into(),
                affected_obligation_ids: Vec::new(),
            },
        )
        .await?;
    (run, _) = store
        .apply_steering(
            &steering_id,
            SteeringApplication {
                expected_steering_revision: steering.revision,
                expected_run_revision: run.revision,
                strategy: "Later strategy".into(),
            },
        )
        .await?;
    let extended = format!("{expected}\n{input}");
    let mut parts = expected_parts;
    parts.push(("s0".into(), expected.len() + 1..extended.len()));
    let mut transaction = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    let actual = acceptance_request(&mut transaction, &id, goal).await?;
    transaction.commit().await?;
    assert_eq!((actual.text, actual.parts), (extended.clone(), parts));
    let recorded: Vec<(i64, String)> = sqlx::query_as("SELECT part,steering_id FROM stateful_acceptance_request_parts WHERE run_id=? ORDER BY part")
        .bind(id.as_str()).fetch_all(&store.pool).await?;
    assert_eq!(
        recorded,
        vec![(1, "s2".into()), (2, "s1".into()), (3, "s0".into())]
    );
    assert_eq!(store.acceptance_request(&id).await?, extended);
    assert_eq!(store.get_run(&id).await?, Some(run));
    store.pool.close().await;
    Ok(())
}

#[tokio::test]
async fn terminal_request_materializes_only_applied_scope_without_changing_run() -> TestResult {
    let goal = "Keep the final report.";
    let (_home, _sqlite, store, id) = fixture(goal).await?;
    let mut run = store.get_run(&id).await?.ok_or("run")?;
    for (name, status) in [
        ("submitted", SteeringStatus::Submitted),
        ("acknowledged", SteeringStatus::Acknowledged),
        ("rejected", SteeringStatus::Rejected),
        ("applied", SteeringStatus::Applied),
    ] {
        let steering_id = SteeringId::parse(name)?;
        let steering = store
            .submit_steering(
                steering_id.clone(),
                NewSteeringInstruction {
                    project_id: run.value.project_id.clone(),
                    run_id: id.clone(),
                    input: format!("{name} instruction."),
                    affected_obligation_ids: Vec::new(),
                },
            )
            .await?;
        match status {
            SteeringStatus::Submitted => {}
            SteeringStatus::Applied => {
                (run, _) = store
                    .apply_steering(
                        &steering_id,
                        SteeringApplication {
                            expected_steering_revision: steering.revision,
                            expected_run_revision: run.revision,
                            strategy: "Applied before cancellation".into(),
                        },
                    )
                    .await?;
            }
            SteeringStatus::Acknowledged | SteeringStatus::Rejected => {
                store
                    .update_steering(
                        &steering_id,
                        SteeringUpdate {
                            expected_revision: steering.revision,
                            status,
                            resulting_strategy_revision: None,
                            reason: (status == SteeringStatus::Rejected)
                                .then(|| "Not applied.".into()),
                        },
                    )
                    .await?;
            }
        }
    }
    let terminal = store
        .update_run(
            &id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Cancelled,
                strategy: None,
                result: Some("Stopped.".into()),
            },
        )
        .await?;
    let expected = format!("{goal}\napplied instruction.");
    assert_eq!(store.acceptance_request(&id).await?, expected);
    let mut transaction = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    let actual = acceptance_request(&mut transaction, &id, goal).await?;
    transaction.commit().await?;
    assert_eq!(
        (actual.text, actual.parts),
        (
            expected.clone(),
            vec![("applied".into(), goal.len() + 1..expected.len())]
        )
    );
    let recorded: Vec<(i64, String)> = sqlx::query_as("SELECT part,steering_id FROM stateful_acceptance_request_parts WHERE run_id=? ORDER BY part")
        .bind(id.as_str()).fetch_all(&store.pool).await?;
    assert_eq!(recorded, vec![(1, "applied".into())]);
    assert_eq!(store.get_run(&id).await?, Some(terminal));
    store.pool.close().await;
    Ok(())
}
