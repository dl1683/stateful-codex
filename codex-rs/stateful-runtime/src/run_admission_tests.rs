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

async fn run(store: &StatefulRunStore, id: &str, project_id: &str) -> StatefulRunId {
    let run_id = StatefulRunId::parse(id).expect("run id");
    store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: project_id.to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: format!("Goal of {id}."),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 600,
                },
            },
        )
        .await
        .expect("run");
    run_id
}

fn admitted(
    admission: Option<(crate::RunTurnAdmission, crate::StatefulRun)>,
) -> Option<(String, u64)> {
    admission.map(|(admission, _)| (admission.run_id.as_str().to_string(), admission.generation))
}

#[tokio::test]
async fn continuation_turns_keep_the_bound_run_and_a_terminal_run_is_never_reopened() {
    let home = TempDir::new().expect("home");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store");
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-1", "thread-1", "turn-0")
                .await
                .expect("no run")
        ),
        None
    );
    let first = run(&store, "run-1", "project-1").await;
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-1", "thread-1", "turn-1")
                .await
                .expect("admit")
        ),
        Some(("run-1".to_string(), 1))
    );
    // A newer active run of the thread does not take over the binding while run-1 is active.
    run(&store, "run-2", "project-1").await;
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-1", "thread-1", "turn-2")
                .await
                .expect("admit")
        ),
        Some(("run-1".to_string(), 1))
    );
    // A retried turn keeps its first admission.
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-1", "thread-1", "turn-1")
                .await
                .expect("retry")
        ),
        Some(("run-1".to_string(), 1))
    );
    // Once run-1 is terminal the thread rebinds to its active run, with a new generation.
    let current = store.get_run(&first).await.expect("read").expect("run-1");
    store
        .update_run(
            &first,
            StatefulRunUpdate {
                expected_revision: current.revision,
                status: StatefulRunStatus::Failed,
                strategy: None,
                result: Some("Stopped.".to_string()),
            },
        )
        .await
        .expect("fail run-1");
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-1", "thread-1", "turn-3")
                .await
                .expect("admit")
        ),
        Some(("run-2".to_string(), 2))
    );
    // Replaying an admitted turn under another project returns nothing and keeps the original.
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-2", "thread-1", "turn-3")
                .await
                .expect("other-project replay")
        ),
        None
    );
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-1", "thread-1", "turn-3")
                .await
                .expect("original replay")
        ),
        Some(("run-2".to_string(), 2))
    );
    // Another project's run never admits this project's turn.
    assert_eq!(
        admitted(
            store
                .admit_run_turn("project-2", "thread-1", "turn-4")
                .await
                .expect("other project")
        ),
        None
    );
}
