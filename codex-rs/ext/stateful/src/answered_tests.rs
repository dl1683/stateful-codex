use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::PARTIAL_RESULT_HEADER;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;
use codex_stateful_runtime::partial_marker;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

const BLOCK: &str =
    "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

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
        trailer_issues: Vec::new(),
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
        Ok(AnsweredRunOutcome::Answered(..))
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

/// A partial ending is disclosed the same way on the normal, the idempotent and the recovered
/// success path, from the durable result; an answered ending stays `Recorded`.
#[tokio::test]
async fn every_success_path_discloses_a_partial_ending() {
    let home = TempDir::new().expect("tempdir");
    let store = store(&home).await;
    let run = running_run(&store).await;
    let commit = AnsweredRunCommit {
        trailer_issues: vec!["tests remain".to_string(), "docs are stale".to_string()],
        ..commit_for(&run)
    };
    let allow = || true;
    let disclosure = TurnFinalizeOutcome::Warning(
        "Stateful: this run ended partial; it is not certified by this terminal answer path. Open or not certified: tests remain; docs are stale".to_string(),
    );
    let normal = store.end_run_answered(&commit, &allow).await;
    assert_eq!(
        resolve(&store, None, &commit, /*authorized*/ true, normal).await,
        disclosure
    );
    let idempotent = store.end_run_answered(&commit, &allow).await;
    assert_eq!(
        resolve(&store, None, &commit, /*authorized*/ true, idempotent).await,
        disclosure
    );
    assert_eq!(
        resolve(
            &store,
            None,
            &commit,
            /*authorized*/ true,
            commit_error()
        )
        .await,
        disclosure
    );
}

/// The disclosure stays within its cap on a character boundary and keeps the omission count.
#[test]
fn the_partial_disclosure_is_bounded() {
    let items = format!("- {}\n- second\n", "é".repeat(700));
    let marked = format!(
        "{PARTIAL_RESULT_HEADER}{items}{}\nThe answer.",
        partial_marker(3)
    );
    let disclosure = partial_disclosure(&marked);
    assert!(
        disclosure.len() <= MAX_DISCLOSURE_BYTES,
        "{}",
        disclosure.len()
    );
    assert!(
        disclosure.ends_with("é [+3 more; see the run result]"),
        "{disclosure}"
    );

    let unmarked = format!("{PARTIAL_RESULT_HEADER}{items}\nThe answer.");
    let disclosure = partial_disclosure(&unmarked);
    assert!(
        disclosure.len() <= MAX_DISCLOSURE_BYTES,
        "{}",
        disclosure.len()
    );
    assert!(
        disclosure.ends_with("é [shortened; see the run result]"),
        "{disclosure}"
    );
}

/// An open issue shaped like the omission marker is disclosed as an issue, never as omitted
/// items, on the normal, the idempotent and the recovered success path.
#[tokio::test]
async fn a_marker_shaped_issue_is_disclosed_as_an_issue_on_every_path() {
    let home = TempDir::new().expect("tempdir");
    let store = store(&home).await;
    let run = running_run(&store).await;
    let commit = AnsweredRunCommit {
        trailer_issues: vec![
            "+3 more (read the acceptance ledger and obligation)".to_string(),
            "\\path\\to\\file is stale".to_string(),
        ],
        ..commit_for(&run)
    };
    let allow = || true;
    let disclosure = TurnFinalizeOutcome::Warning(
        "Stateful: this run ended partial; it is not certified by this terminal answer path. Open or not certified: +3 more (read the acceptance ledger and obligation); \\path\\to\\file is stale".to_string(),
    );
    let normal = store.end_run_answered(&commit, &allow).await;
    assert_eq!(
        resolve(&store, None, &commit, /*authorized*/ true, normal).await,
        disclosure
    );
    let idempotent = store.end_run_answered(&commit, &allow).await;
    assert_eq!(
        resolve(&store, None, &commit, /*authorized*/ true, idempotent).await,
        disclosure
    );
    assert_eq!(
        resolve(
            &store,
            None,
            &commit,
            /*authorized*/ true,
            commit_error()
        )
        .await,
        disclosure
    );
}
