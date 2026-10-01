use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulRunUpdate;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::fail_run_after_task_panic;
use crate::services::ProjectIntelligenceServices;

async fn run_status_after_task_panic(paused_first: bool) -> StatefulRunStatus {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let store = services.runtime().await.expect("runtime opens");
    let run_id = StatefulRunId::parse("run-panicked").expect("valid run ID");
    let run = store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Finish the parser.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run is created");
    if paused_first {
        store
            .update_run(
                &run_id,
                StatefulRunUpdate {
                    expected_revision: run.revision,
                    status: StatefulRunStatus::Paused,
                    strategy: None,
                    result: None,
                },
            )
            .await
            .expect("run pauses");
    }
    fail_run_after_task_panic(&services, /*event_sink*/ None, &run_id).await;
    store
        .get_run(&run_id)
        .await
        .expect("run reads")
        .expect("run exists")
        .status
}

#[tokio::test]
async fn task_panic_fails_a_running_run_but_never_overrides_a_user_control() {
    assert_eq!(
        (
            run_status_after_task_panic(/*paused_first*/ false).await,
            run_status_after_task_panic(/*paused_first*/ true).await,
        ),
        (StatefulRunStatus::Failed, StatefulRunStatus::Paused)
    );
}
