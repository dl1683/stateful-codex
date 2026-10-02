use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::NewStatefulRun;
use crate::RunBudget;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunUpdate;
use crate::WorkflowMode;

fn new_run(project_id: &str, thread_id: &str) -> NewStatefulRun {
    NewStatefulRun {
        project_id: project_id.to_string(),
        thread_ids: vec![thread_id.to_string()],
        goal: "Continue the work.".to_string(),
        mode: WorkflowMode::Collaborative,
        budget: RunBudget {
            max_continuations: 1,
            max_elapsed_seconds: 600,
        },
    }
}

#[tokio::test]
async fn run_history_includes_terminal_runs_and_finds_the_latest_project_run() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(temp_dir.path().abs()))
        .await
        .expect("store opens");
    let first = store
        .create_run(
            StatefulRunId::parse("run-first").expect("valid run ID"),
            new_run("project-1", "thread-1"),
        )
        .await
        .expect("first run inserts");
    let first = store
        .update_run(
            &first.id,
            StatefulRunUpdate {
                expected_revision: first.revision,
                status: StatefulRunStatus::Completed,
                strategy: None,
                result: Some("Done.".to_string()),
            },
        )
        .await
        .expect("first run completes");
    let second = store
        .create_run(
            StatefulRunId::parse("run-second").expect("valid run ID"),
            new_run("project-1", "thread-1"),
        )
        .await
        .expect("second run inserts");
    store
        .create_run(
            StatefulRunId::parse("run-other").expect("valid run ID"),
            new_run("project-2", "thread-2"),
        )
        .await
        .expect("other project's run inserts");

    assert_eq!(
        store
            .runs_for_thread("thread-1", /*max_results*/ 20)
            .await
            .expect("thread runs load"),
        vec![second.clone(), first]
    );
    assert_eq!(
        store
            .latest_project_run("project-1")
            .await
            .expect("latest run loads"),
        Some(second)
    );
    assert_eq!(
        store
            .latest_project_run("project-3")
            .await
            .expect("empty project loads"),
        None
    );
}
