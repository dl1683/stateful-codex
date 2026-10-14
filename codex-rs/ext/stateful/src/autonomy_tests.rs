use std::sync::Arc;
use std::time::Duration;

use codex_extension_api::ExtensionData;
use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_extension_api::TurnLifecycleContributor;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
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

use super::ActiveRunTurn;
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

/// The receipt read cannot be stalled through the public API (SQLite readers never wait on
/// the writer), so this drives the commit gate's outcome directly: when an abort wins, the
/// run's single warning is not spent, and its next status-only final still warns once.
#[tokio::test]
async fn an_aborted_finalizer_leaves_the_run_its_warning() {
    let state_home = TempDir::new().expect("temporary state home");
    let sqlite = SqliteConfig::new_for_testing(state_home.path().abs());
    let extension = crate::StatefulExtension {
        projects: Arc::new(codex_thread_store::InMemoryThreadStore::default()),
        services: Some(ProjectIntelligenceServices::new(sqlite)),
        event_sink: None,
        autonomous: None,
        attribution: Default::default(),
        visible_root: Default::default(),
        run_activity: Default::default(),
        run_commits: Default::default(),
        turn_progress: Default::default(),
    };
    let (session_store, thread_store) = (ExtensionData::new("s"), ExtensionData::new("thread-1"));
    thread_store.insert(crate::SelectedThread::new("thread-1"));
    let mut outcomes = Vec::new();
    for (turn_id, authorized) in [("turn-1", false), ("turn-2", true), ("turn-3", true)] {
        let run_id = StatefulRunId::parse("run-1").expect("run id");
        thread_store.insert(ActiveRunTurn {
            run_id,
            turn_id: turn_id.to_string(),
        });
        let turn_store = ExtensionData::new(turn_id);
        let content = vec![AgentMessageContent::Text {
            text: "Done.".to_string(),
        }];
        turn_store.insert(crate::answered::LastAgentMessage::from_item(
            &AgentMessageItem {
                id: "final".to_string(),
                content,
                phase: None,
                memory_citation: None,
                delivery: None,
                questions: None,
            },
        ));
        let authorize = move || authorized;
        let input = TurnFinalizeInput {
            turn_id,
            last_agent_message: "Done.",
            authorize_commit: &authorize,
            session_store: &session_store,
            thread_store: &thread_store,
            turn_store: &turn_store,
        };
        outcomes.push(extension.on_turn_finalize(input).await);
    }
    let warning = "This final reply may omit the deliverable: it contains only a status line.";
    assert_eq!(
        outcomes,
        [
            TurnFinalizeOutcome::NotHandled,
            TurnFinalizeOutcome::Warning(warning.to_string()),
            TurnFinalizeOutcome::NotHandled,
        ]
    );
}

/// G1: the extension hook consumes shared progress and respects explicit answer/steering.
#[tokio::test]
async fn finalization_hook_respects_recorded_progress_and_steering() {
    for pending in [false, true] {
        let home = TempDir::new().expect("home");
        let services =
            ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
        let store = services.runtime().await.expect("runtime");
        let run = store
            .create_run(
                StatefulRunId::parse("run-1").expect("id"),
                NewStatefulRun {
                    project_id: "project-1".into(),
                    thread_ids: vec!["thread-1".into()],
                    goal: "What does parse_config return?".into(),
                    mode: WorkflowMode::Autonomous,
                    budget: RunBudget {
                        max_continuations: 1,
                        max_elapsed_seconds: 600,
                    },
                },
            )
            .await
            .expect("run");
        if pending {
            store
                .submit_steering(
                    codex_stateful_runtime::SteeringId::parse("pending").expect("id"),
                    codex_stateful_runtime::NewSteeringInstruction {
                        project_id: "project-1".into(),
                        run_id: run.id.clone(),
                        input: "Check the revised scope.".into(),
                        affected_obligation_ids: vec![],
                    },
                )
                .await
                .expect("steering");
        }
        let extension = crate::StatefulExtension {
            projects: Arc::new(codex_thread_store::InMemoryThreadStore::default()),
            services: Some(services.clone()),
            event_sink: None,
            autonomous: None,
            attribution: Default::default(),
            visible_root: Default::default(),
            run_activity: Default::default(),
            run_commits: Default::default(),
            turn_progress: Default::default(),
        };
        let session = ExtensionData::new("session");
        let thread = ExtensionData::new("thread-1");
        let turn = ExtensionData::new("turn-1");
        thread.insert(crate::SelectedThread::new("thread-1"));
        thread.insert(ActiveRunTurn {
            run_id: run.id.clone(),
            turn_id: "turn-1".into(),
        });
        for explicit in [false, true] {
            extension.turn_progress.record("turn-1", &run.id);
            let answer = if explicit {
                "It returns a Result.\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]"
            } else {
                "It returns a Result."
            };
            turn.insert(crate::answered::LastAgentMessage::from_item(
                &AgentMessageItem {
                    id: "final".into(),
                    content: vec![AgentMessageContent::Text {
                        text: answer.into(),
                    }],
                    phase: None,
                    memory_citation: None,
                    delivery: None,
                    questions: None,
                },
            ));
            let outcome = extension
                .on_turn_finalize(TurnFinalizeInput {
                    turn_id: "turn-1",
                    last_agent_message: answer,
                    authorize_commit: &|| true,
                    session_store: &session,
                    thread_store: &thread,
                    turn_store: &turn,
                })
                .await;
            let status = store
                .get_run(&run.id)
                .await
                .expect("read")
                .expect("run")
                .status;
            if !explicit {
                assert_eq!(
                    (outcome, status),
                    (TurnFinalizeOutcome::NotHandled, StatefulRunStatus::Running)
                );
            } else if pending {
                assert!(matches!(outcome, TurnFinalizeOutcome::Warning(_)));
                assert_eq!(status, StatefulRunStatus::Running);
            } else {
                assert_eq!(
                    (outcome, status),
                    (TurnFinalizeOutcome::Recorded, StatefulRunStatus::Answered)
                );
            }
        }
    }
}
