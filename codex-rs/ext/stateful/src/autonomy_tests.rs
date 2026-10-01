use std::sync::Arc;
use std::time::Duration;

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

use super::AutonomousContinuation;
use super::AutonomousContinuationFuture;
use super::AutonomousContinuationOutcome;
use super::AutonomousContinuationRequest;
use super::AutonomousContinuationSink;
use super::PendingContinuation;
use super::RunAdmissionFence;
use super::attempt_continuation;
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

/// Holds the continuation's turn start open until the test releases it.
struct GatedSink {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl AutonomousContinuationSink for GatedSink {
    fn continue_run<'a>(
        &'a self,
        _request: AutonomousContinuationRequest,
    ) -> AutonomousContinuationFuture<'a> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(AutonomousContinuationOutcome::Started)
        })
    }
}

#[tokio::test]
async fn cancel_waits_for_an_admitted_continuation_to_start_its_turn() {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let run_id = StatefulRunId::parse("run-autonomous").expect("valid run ID");
    services
        .runtime()
        .await
        .expect("runtime opens")
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Finish the parser.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run is created");
    let sink = Arc::new(GatedSink {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let admission = RunAdmissionFence::default();
    let autonomous = AutonomousContinuation::new(
        "owner-1".to_string(),
        Arc::clone(&sink) as Arc<dyn AutonomousContinuationSink>,
        admission.clone(),
    );
    let continuation = tokio::spawn({
        let services = services.clone();
        async move {
            attempt_continuation(
                &services,
                &autonomous,
                /*event_sink*/ None,
                &PendingContinuation {
                    thread_id: "thread-1".to_string(),
                    run_id,
                    previous_turn_id: "turn-1".to_string(),
                },
            )
            .await
        }
    });
    sink.entered.notified().await;

    // A cancel cannot read active turns while the claimed continuation is starting its turn.
    let blocked = tokio::time::timeout(Duration::from_millis(200), admission.lock())
        .await
        .is_err();
    sink.release.notify_one();
    let started = continuation.await.expect("continuation task joins");
    let admitted_after_start = tokio::time::timeout(Duration::from_secs(5), admission.lock())
        .await
        .is_ok();
    assert_eq!((blocked, started, admitted_after_start), (true, None, true));
}
