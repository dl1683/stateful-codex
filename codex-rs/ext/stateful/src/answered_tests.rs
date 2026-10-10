use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

const BLOCK: &str =
    "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

/// Only an answer followed by a well-formed, trailing, ready block is admitted; a
/// declaration-only message, whitespace before the block, text after it, and every other
/// disposition keep the run's ordinary route.
#[test]
fn readiness_admits_only_a_ready_block_trailing_real_answer_text() {
    assert_eq!(
        readiness(&format!("It returns Result<Config, Error>.\n\n{BLOCK}\n")),
        Readiness::Answered("It returns Result<Config, Error>.")
    );
    for (message, reason) in [
        ("It returns a Result.".to_string(), "the final message has no outcome block"),
        (BLOCK.to_string(), "no answer precedes the outcome block"),
        (format!(" \n\t\n{BLOCK}"), "no answer precedes the outcome block"),
        (
            format!("It returns a Result.\n{BLOCK}\nAnything else?"),
            "text follows the outcome block",
        ),
        (
            format!("It returns a Result. {BLOCK}"),
            "the outcome block does not start on its own line",
        ),
        (
            format!("A.\n{BLOCK}\nB.\n{BLOCK}"),
            "the final message has more than one outcome block",
        ),
        (
            "I will read the parser next.\n[stateful-outcome]\ndisposition: continue\nopen-issues: none\n[/stateful-outcome]".to_string(),
            "the model declared that work continues",
        ),
        (
            "I cannot reach the file.\n[stateful-outcome]\ndisposition: blocked\nopen-issues:\n- The file is missing.\n[/stateful-outcome]".to_string(),
            "the model declared that it is blocked",
        ),
        (
            "It probably returns a Result.\n[stateful-outcome]\ndisposition: answer\nopen-issues:\n- I did not read the source.\n[/stateful-outcome]".to_string(),
            "the answer declares open issues",
        ),
        (
            "It returns a Result.\n[stateful-outcome]\ndisposition: answer\nopen-issues: none".to_string(),
            "the outcome block is not closed",
        ),
    ] {
        assert_eq!(readiness(&message), Readiness::NotAnswered(reason), "{message}");
    }
}

async fn store(home: &TempDir) -> StatefulRunStore {
    StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens")
}

async fn running_run(store: &StatefulRunStore) -> StatefulRun {
    store
        .create_run(
            StatefulRunId::parse("run-1").expect("run id"),
            NewStatefulRun {
                project_id: "project".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "What does parse_config return?".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await
        .expect("run created")
}

fn commit_for(run: &StatefulRun) -> AnsweredRunCommit {
    AnsweredRunCommit {
        run_id: run.id.clone(),
        thread_id: "thread-1".to_string(),
        turn_id: "turn-1".to_string(),
        answer: format!("It returns a Result.\n{BLOCK}"),
        result: "It returns a Result.".to_string(),
    }
}

fn commit_error() -> Result<AnsweredRunOutcome, StatefulRunStoreError> {
    Err(StatefulRunStoreError::ConcurrentMutation)
}

/// An error after authorization leaves the COMMIT outcome unknown: the record is re-read, and
/// the result is reported as answered, not saved, or unknown, never as a cancellation.
#[tokio::test]
async fn an_unknown_commit_outcome_is_re_read() {
    // The commit landed although its result was an error.
    let home = TempDir::new().expect("tempdir");
    let landed = store(&home).await;
    let run = running_run(&landed).await;
    let commit = commit_for(&run);
    let allow = || true;
    assert!(matches!(
        landed.end_run_answered(&commit, &allow).await,
        Ok(AnsweredRunOutcome::Answered(_))
    ));
    assert_eq!(
        resolve(
            &landed,
            None,
            &commit,
            /*authorized*/ true,
            commit_error()
        )
        .await,
        TurnFinalizeOutcome::Recorded
    );

    // The commit did not land.
    let home = TempDir::new().expect("tempdir");
    let lost = store(&home).await;
    let run = running_run(&lost).await;
    let commit = commit_for(&run);
    assert_eq!(
        resolve(&lost, None, &commit, /*authorized*/ true, commit_error()).await,
        TurnFinalizeOutcome::Warning(
            "Ending the Stateful run with this answer was not saved (run changed during a guarded update), so it continues as usual.".to_string()
        )
    );
    assert_eq!(
        lost.get_run(&run.id)
            .await
            .expect("read")
            .expect("run")
            .status,
        StatefulRunStatus::Running
    );
    // An error before authorization never reached COMMIT.
    assert_eq!(
        resolve(&lost, None, &commit, /*authorized*/ false, commit_error()).await,
        TurnFinalizeOutcome::Warning(
            "The Stateful run could not be ended with this answer, so it continues as usual: run changed during a guarded update".to_string()
        )
    );

    // The record cannot be re-read: the outcome stays unknown and says so.
    let unknown = after_unknown_commit(
        "turn-1",
        &StatefulRunStoreError::ConcurrentMutation,
        Err(StatefulRunStoreError::RunNotFound("run-1".to_string())),
    )
    .expect_err("an unknown outcome is reported");
    assert_eq!(
        unknown,
        "Ending the Stateful run with this answer may or may not have been saved (run changed during a guarded update), and its state could not be re-read (run not found: run-1). Read the run before relying on how it ended."
    );
}
