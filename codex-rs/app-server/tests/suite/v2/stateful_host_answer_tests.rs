//! Host-ended Autonomous answers through the public API: a new Autonomous run whose first
//! task only answers, and declares itself ready, completes with the exact answer and an
//! explicit unverified basis; every other order keeps the ordinary route.

use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunSetModeParams;
use codex_app_server_protocol::StatefulRunSetModeResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadShellCommandParams;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_state::SqliteConfig;
use codex_stateful_runtime::HOST_ANSWER_BASIS;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStore;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const QUESTION: &str = "What does parse_config return?";
const READY: &str = "parse_config returns Result<Config, Error>.\n\n[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

struct Harness {
    codex_home: TempDir,
    _project_root: TempDir,
    responses_server: wiremock::MockServer,
    server: TestAppServer,
    project_id: String,
    thread_id: String,
}

async fn harness() -> Result<Harness> {
    harness_with(|config| config).await
}

async fn harness_with(
    configure: impl FnOnce(MockResponsesConfig) -> MockResponsesConfig,
) -> Result<Harness> {
    harness_on(configure, HostEnvironment::Executor).await
}

/// Where turns run: `Host` keeps the app-server's local environment, which
/// `thread/shellCommand` needs.
enum HostEnvironment {
    Executor,
    Host,
}

async fn harness_on(
    configure: impl FnOnce(MockResponsesConfig) -> MockResponsesConfig,
    environment: HostEnvironment,
) -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::init_repository(project_root.path())?;
    configure(
        MockResponsesConfig::new(&responses_server.uri())
            .with_sandbox_mode("read-only")
            .enable_feature(Feature::Sqlite),
    )
    .write(codex_home.path())?;
    let builder = TestAppServer::builder().with_codex_home(codex_home.path());
    let builder = match &environment {
        HostEnvironment::Executor => builder,
        HostEnvironment::Host => builder.without_auto_env(),
    };
    let mut server = builder.build_initialized().await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Host answers".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "host-answer-project".to_string(),
            },
        })
        .await?;
    let thread_params = ThreadStartParams {
        project_id: Some(project.project.id.clone()),
        cwd: Some(project_root.path().to_string_lossy().to_string()),
        ..Default::default()
    };
    let thread: ThreadStartResponse = match environment {
        HostEnvironment::Executor => server.start_thread(thread_params).await?,
        HostEnvironment::Host => {
            server
                .request(|request_id| ClientRequest::ThreadStart {
                    request_id,
                    params: thread_params,
                })
                .await?
        }
    };
    Ok(Harness {
        codex_home,
        _project_root: project_root,
        responses_server,
        server,
        project_id: project.project.id,
        thread_id: thread.thread.id,
    })
}

impl Harness {
    async fn start_run(&mut self, mode: StatefulWorkflowMode, key: &str) -> Result<StatefulRun> {
        let params = StatefulRunStartParams {
            project_id: self.project_id.clone(),
            thread_id: self.thread_id.clone(),
            goal: QUESTION.to_string(),
            mode,
            budget: StatefulRunBudget {
                max_continuations: 1,
                max_elapsed_seconds: 3_600,
            },
            idempotency_key: key.to_string(),
        };
        let started: StatefulRunStartResponse = self
            .server
            .request(|request_id| ClientRequest::StatefulRunStart { request_id, params })
            .await?;
        Ok(started.run)
    }

    async fn start_turn(&mut self, text: &str) -> Result<String> {
        let request_id = self
            .server
            .send_turn_start_request(TurnStartParams {
                thread_id: self.thread_id.clone(),
                input: vec![UserInput::Text {
                    text: text.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        let started: TurnStartResponse = self.server.read_response(request_id).await?;
        Ok(started.turn.id)
    }

    async fn turn_completed(&mut self, turn_id: &str) -> Result<TurnCompletedNotification> {
        loop {
            let completed: TurnCompletedNotification =
                self.server.read_notification("turn/completed").await?;
            if completed.turn.id == turn_id {
                return Ok(completed);
            }
        }
    }

    async fn turn(&mut self, text: &str) -> Result<TurnCompletedNotification> {
        let turn_id = self.start_turn(text).await?;
        self.turn_completed(&turn_id).await
    }

    async fn read(&mut self, run_id: &str) -> Result<StatefulRunReadResponse> {
        let run_id = run_id.to_string();
        self.server
            .request(|request_id| ClientRequest::StatefulRunRead {
                request_id,
                params: StatefulRunReadParams {
                    run_id: Some(run_id),
                    thread_id: None,
                },
            })
            .await
    }

    /// Requests a user shell command and returns its refusal, if it was refused.
    async fn shell(&mut self, command: &str) -> Result<Option<JSONRPCError>> {
        let request_id = self
            .server
            .send_thread_shell_command_request(ThreadShellCommandParams {
                thread_id: self.thread_id.clone(),
                command: command.to_string(),
                timeout_ms: None,
            })
            .await?;
        let message = tokio::time::timeout(
            Duration::from_secs(10),
            self.server
                .read_stream_until_response_or_error(RequestId::Integer(request_id)),
        )
        .await??;
        Ok(message.err())
    }

    /// Waits until the turn's final assistant message completed: the task then finalizes.
    async fn wait_for_answer_item(&mut self) -> Result<()> {
        self.server
            .read_stream_until_matching_notification("agent message completed", |notification| {
                notification.method == "item/completed"
                    && notification
                        .params
                        .as_ref()
                        .and_then(|params| params.get("item"))
                        .and_then(|item| item.get("type"))
                        .and_then(serde_json::Value::as_str)
                        == Some("agentMessage")
            })
            .await?;
        Ok(())
    }

    async fn wait_for_model_requests(&self, mock: &responses::ResponseMock, count: usize) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while mock.requests().len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("model request received");
    }

    /// Holds the run store's write lock, so a finalizer waits at its terminal transaction.
    async fn hold_run_store(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        let sqlite = SqliteConfig::new_for_testing(self.codex_home.path().abs());
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join("stateful_runtime_1.sqlite"))
            .await?;
        Ok(pool.begin_with("BEGIN IMMEDIATE").await?)
    }

    async fn ledger_criteria(&self, run_id: &str) -> Result<usize> {
        let store =
            StatefulRunStore::open(&SqliteConfig::new_for_testing(self.codex_home.path().abs()))
                .await?;
        Ok(store
            .acceptance_ledger(&StatefulRunId::parse(run_id)?)
            .await?
            .criteria
            .len())
    }
}

fn held(body: String) -> wiremock::ResponseTemplate {
    responses::sse_response(body).set_delay(Duration::from_secs(2))
}

/// T01/T36: a fresh Autonomous run whose first task only answers and declares no open
/// issues completes when that task ends: the exact answer and the unverified basis are
/// durable and readable, no criterion was fabricated, and no continuation was requested.
#[tokio::test]
async fn fresh_pure_answer_completes_with_an_unverified_basis() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness
        .start_run(StatefulWorkflowMode::Autonomous, "pure-answer")
        .await?;
    let before = harness.read(&run.id).await?;
    assert_eq!(before.run.expect("run").status, StatefulRunStatus::Running);
    assert_eq!(before.host_answer, None);
    let mock = responses::mount_sse_once(&harness.responses_server, message("r1", READY)).await;

    let completed = harness.turn(QUESTION).await?;
    assert_eq!(completed.turn.status, TurnStatus::Completed);

    let read = harness.read(&run.id).await?;
    let completed_run = read.run.expect("run");
    assert_eq!(completed_run.status, StatefulRunStatus::Completed);
    assert_eq!(completed_run.result.as_deref(), Some(READY));
    let host_answer = read.host_answer.expect("host answer recorded");
    assert_eq!(host_answer.turn_id, completed.turn.id);
    assert_eq!(host_answer.answer, READY);
    assert_eq!(host_answer.basis, HOST_ANSWER_BASIS);
    assert!(host_answer.basis.contains("not host-verified"));
    assert_eq!(
        read.recovery
            .expect("recovery")
            .last_continuation_claimed_at,
        None
    );
    assert_eq!(mock.requests().len(), 1);
    assert_eq!(harness.ledger_criteria(&run.id).await?, 0);
    Ok(())
}

/// T30: a final answer without the ready declaration, one that says work continues, and
/// one that declares open issues all keep the ordinary route.
#[tokio::test]
async fn answers_without_the_ready_declaration_keep_the_ordinary_route() -> Result<()> {
    for text in [
        "parse_config returns Result<Config, Error>.",
        "I will look at the parser next.\n\n[stateful-outcome]\ndisposition: continue\nopen-issues: none\n[/stateful-outcome]",
        "It probably returns a Result.\n\n[stateful-outcome]\ndisposition: answer\nopen-issues:\n- I did not read the source.\n[/stateful-outcome]",
    ] {
        let mut harness = harness().await?;
        let run = harness
            .start_run(StatefulWorkflowMode::Autonomous, "not-ready")
            .await?;
        responses::mount_sse_once(&harness.responses_server, message("r1", text)).await;
        harness.turn(QUESTION).await?;
        let read = harness.read(&run.id).await?;
        assert_ne!(
            read.run.expect("run").status,
            StatefulRunStatus::Completed,
            "{text}"
        );
        assert_eq!(read.host_answer, None, "{text}");
    }
    Ok(())
}

/// T03: any tool call in the first task (here a call the host rejects) keeps the ordinary
/// route even when the final answer declares itself ready.
#[tokio::test]
async fn a_tool_call_keeps_the_ordinary_route() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness
        .start_run(StatefulWorkflowMode::Autonomous, "tool-call")
        .await?;
    responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("r1"),
                responses::ev_function_call("call-1", "no_such_tool", "{}"),
                responses::ev_completed("r1"),
            ]),
            message("r2", READY),
        ],
    )
    .await;
    harness.turn(QUESTION).await?;
    let read = harness.read(&run.id).await?;
    assert_ne!(read.run.expect("run").status, StatefulRunStatus::Completed);
    assert_eq!(read.host_answer, None);
    Ok(())
}

/// T14 (legacy notify): a configured executable hook keeps the ordinary route.
#[tokio::test]
async fn a_configured_hook_keeps_the_ordinary_route() -> Result<()> {
    let notify = if cfg!(windows) {
        "notify = [\"cmd\", \"/c\", \"exit 0\"]\n"
    } else {
        "notify = [\"true\"]\n"
    };
    let mut harness = harness_with(|config| config.with_root_config(notify)).await?;
    let run = harness
        .start_run(StatefulWorkflowMode::Autonomous, "hook")
        .await?;
    responses::mount_sse_once(&harness.responses_server, message("r1", READY)).await;
    harness.turn(QUESTION).await?;
    let read = harness.read(&run.id).await?;
    assert_ne!(read.run.expect("run").status, StatefulRunStatus::Completed);
    assert_eq!(read.host_answer, None);
    Ok(())
}

/// T09/T12: while the run's first task answers, a user shell command is refused with an
/// actionable error and nothing starts; after the answer completed the run, a command runs.
#[tokio::test]
async fn shell_is_refused_while_the_run_answers() -> Result<()> {
    let mut harness = harness_on(|config| config, HostEnvironment::Host).await?;
    let run = harness
        .start_run(StatefulWorkflowMode::Autonomous, "shell-answering")
        .await?;
    let mock =
        responses::mount_response_once(&harness.responses_server, held(message("r1", READY))).await;
    let turn_id = harness.start_turn(QUESTION).await?;
    harness.wait_for_model_requests(&mock, 1).await;

    let refusal = harness
        .shell("echo beside-the-answer")
        .await?
        .expect("refused while answering");
    assert!(
        refusal
            .error
            .message
            .contains("no command can start beside it"),
        "{refusal:?}"
    );
    harness.turn_completed(&turn_id).await?;
    assert_eq!(
        harness.read(&run.id).await?.run.expect("run").status,
        StatefulRunStatus::Completed
    );
    assert_eq!(harness.shell("echo after-the-answer").await?, None);
    Ok(())
}

/// T10: a user shell command that arrives while the finalizer waits for the run store is
/// refused before spawn; the answer then completes the run.
#[tokio::test]
async fn shell_is_refused_while_the_answer_finalizes() -> Result<()> {
    let mut harness = harness_on(|config| config, HostEnvironment::Host).await?;
    let run = harness
        .start_run(StatefulWorkflowMode::Autonomous, "shell-finalizing")
        .await?;
    let mock =
        responses::mount_response_once(&harness.responses_server, held(message("r1", READY))).await;
    let turn_id = harness.start_turn(QUESTION).await?;
    harness.wait_for_model_requests(&mock, 1).await;
    let lock = harness.hold_run_store().await?;
    harness
        .server
        .read_stream_until_notification_message("item/completed")
        .await?;
    let refusal = harness.shell("echo beside-the-finalizer").await?;
    assert!(refusal.is_some(), "refused while finalizing");
    lock.rollback().await?;
    harness.turn_completed(&turn_id).await?;
    let read = harness.read(&run.id).await?;
    assert_eq!(read.run.expect("run").status, StatefulRunStatus::Completed);
    assert!(read.host_answer.is_some());
    Ok(())
}

/// T19/T20: an interrupt during inference, or while the finalizer waits for the run store,
/// leaves the run Running with no host answer.
#[tokio::test]
async fn interrupts_before_the_commit_leave_the_run_running() -> Result<()> {
    for wait_for_finalizer in [false, true] {
        let mut harness = harness().await?;
        let run = harness
            .start_run(StatefulWorkflowMode::Autonomous, "interrupt")
            .await?;
        let mock =
            responses::mount_response_once(&harness.responses_server, held(message("r1", READY)))
                .await;
        let turn_id = harness.start_turn(QUESTION).await?;
        harness.wait_for_model_requests(&mock, 1).await;
        let lock = if wait_for_finalizer {
            let lock = harness.hold_run_store().await?;
            harness.wait_for_answer_item().await?;
            Some(lock)
        } else {
            None
        };
        let thread_id = harness.thread_id.clone();
        let interrupt = turn_id.clone();
        let request_id = harness
            .server
            .send_turn_interrupt_request(TurnInterruptParams {
                thread_id,
                turn_id: interrupt,
            })
            .await?;
        let completed = harness.turn_completed(&turn_id).await?;
        assert_eq!(completed.turn.status, TurnStatus::Interrupted);
        harness
            .server
            .read_stream_until_response_message(RequestId::Integer(request_id))
            .await?;
        if let Some(lock) = lock {
            lock.rollback().await?;
        }
        let read = harness.read(&run.id).await?;
        assert_ne!(
            read.run.expect("run").status,
            StatefulRunStatus::Completed,
            "finalizer wait: {wait_for_finalizer}"
        );
        assert_eq!(read.host_answer, None);
    }
    Ok(())
}

/// T25/T26: a run converted to Autonomous, and a run started while a turn is running, get
/// no candidacy: their later pure answers keep the ordinary route.
#[tokio::test]
async fn converted_and_mid_turn_runs_get_no_candidacy() -> Result<()> {
    // Converted from Collaborative.
    let mut converted_harness = harness().await?;
    let run = converted_harness
        .start_run(StatefulWorkflowMode::Collaborative, "converted")
        .await?;
    let run_id = run.id.clone();
    let converted: StatefulRunSetModeResponse = converted_harness
        .server
        .request(|request_id| ClientRequest::StatefulRunSetMode {
            request_id,
            params: StatefulRunSetModeParams {
                run_id,
                expected_revision: run.revision,
                mode: StatefulWorkflowMode::Autonomous,
            },
        })
        .await?;
    assert_eq!(converted.run.mode, StatefulWorkflowMode::Autonomous);
    responses::mount_sse_once(&converted_harness.responses_server, message("r1", READY)).await;
    converted_harness.turn(QUESTION).await?;
    let read = converted_harness.read(&run.id).await?;
    assert_ne!(read.run.expect("run").status, StatefulRunStatus::Completed);
    assert_eq!(read.host_answer, None);

    // Started while an ordinary turn runs on the thread.
    let mut harness = harness().await?;
    let mock = responses::mount_response_sequence(
        &harness.responses_server,
        vec![
            held(message("r1", "Hello.")),
            responses::sse_response(message("r2", READY)),
        ],
    )
    .await;
    let earlier = harness.start_turn("Say hello.").await?;
    harness.wait_for_model_requests(&mock, 1).await;
    let run = harness
        .start_run(StatefulWorkflowMode::Autonomous, "mid-turn")
        .await?;
    harness.turn_completed(&earlier).await?;
    harness.turn(QUESTION).await?;
    let read = harness.read(&run.id).await?;
    assert_ne!(read.run.expect("run").status, StatefulRunStatus::Completed);
    assert_eq!(read.host_answer, None);
    Ok(())
}
