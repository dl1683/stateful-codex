//! "Answered, unverified" through the public API: an Autonomous turn whose final answer ends
//! with the ready outcome block ends its run as Answered (never Completed), with the exact
//! answer and an explicit unverified basis; anything that is not a genuine, ready final
//! answer keeps the ordinary route.

use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
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
use codex_stateful_runtime::ANSWERED_UNVERIFIED_BASIS;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

const QUESTION: &str = "What does parse_config return?";
const ANSWER: &str = "parse_config returns Result<Config, Error>.";
const BLOCK: &str =
    "[stateful-outcome]\ndisposition: answer\nopen-issues: none\n[/stateful-outcome]";

fn ready() -> String {
    format!("{ANSWER}\n\n{BLOCK}")
}

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

fn commentary(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "message",
                "role": "assistant",
                "id": id,
                "phase": "commentary",
                "content": [{"type": "output_text", "text": text}]
            }
        }),
        responses::ev_completed(id),
    ])
}

fn held(body: String) -> wiremock::ResponseTemplate {
    responses::sse_response(body).set_delay(Duration::from_secs(2))
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
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::init_repository(project_root.path())?;
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode("read-only")
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Answered runs".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "answered-project".to_string(),
            },
        })
        .await?;
    let thread: ThreadStartResponse = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            cwd: Some(project_root.path().to_string_lossy().to_string()),
            ..Default::default()
        })
        .await?;
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
    async fn start_run(&mut self, mode: StatefulWorkflowMode) -> Result<StatefulRun> {
        let params = StatefulRunStartParams {
            project_id: self.project_id.clone(),
            thread_id: self.thread_id.clone(),
            goal: QUESTION.to_string(),
            mode,
            budget: StatefulRunBudget {
                max_continuations: 1,
                max_elapsed_seconds: 3_600,
            },
            idempotency_key: "answered-run".to_string(),
        };
        let started: StatefulRunStartResponse = self
            .server
            .request(|request_id| ClientRequest::StatefulRunStart { request_id, params })
            .await?;
        Ok(started.run)
    }

    async fn start_turn(&mut self) -> Result<String> {
        let request_id = self
            .server
            .send_turn_start_request(TurnStartParams {
                thread_id: self.thread_id.clone(),
                input: vec![UserInput::Text {
                    text: QUESTION.to_string(),
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

    async fn turn(&mut self) -> Result<TurnCompletedNotification> {
        let turn_id = self.start_turn().await?;
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

    async fn status(&mut self, run_id: &str) -> Result<StatefulRunStatus> {
        Ok(self.read(run_id).await?.run.expect("run").status)
    }

    /// Holds the run store's write lock, so a finalizer waits at its terminal transaction.
    async fn hold_run_store(&self) -> Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        let sqlite = SqliteConfig::new_for_testing(self.codex_home.path().abs());
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join("stateful_runtime_1.sqlite"))
            .await?;
        Ok(pool.begin_with("BEGIN IMMEDIATE").await?)
    }
}

/// A ready final answer ends the Autonomous run as Answered, not Completed: the exact answer
/// and the unverified basis are durable, the result is the answer text, no continuation is
/// requested, and all of it reads back the same after an app-server restart.
#[tokio::test]
async fn a_ready_answer_ends_the_run_answered_across_restart() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
    assert_eq!(harness.read(&run.id).await?.host_answer, None);
    let mock = responses::mount_sse_once(&harness.responses_server, message("r1", &ready())).await;

    let completed = harness.turn().await?;
    assert_eq!(completed.turn.status, TurnStatus::Completed);

    let read = harness.read(&run.id).await?;
    let answered = read.run.expect("run");
    assert_eq!(answered.status, StatefulRunStatus::Answered);
    assert_eq!(answered.result.as_deref(), Some(ANSWER));
    let host_answer = read.host_answer.expect("answer recorded");
    assert_eq!(host_answer.turn_id, completed.turn.id);
    assert_eq!(host_answer.answer, ready());
    assert_eq!(host_answer.basis, ANSWERED_UNVERIFIED_BASIS);
    assert_eq!(
        read.recovery
            .expect("recovery")
            .last_continuation_claimed_at,
        None
    );
    assert_eq!(mock.requests().len(), 1);

    // Restart the app-server on the same home.
    let restarted = TestAppServer::builder()
        .with_codex_home(harness.codex_home.path())
        .build_initialized()
        .await?;
    drop(std::mem::replace(&mut harness.server, restarted));
    let reread = harness.read(&run.id).await?;
    assert_eq!(reread.run.expect("run"), answered);
    assert_eq!(reread.host_answer.expect("answer kept"), host_answer);
    Ok(())
}

/// An Autonomous run that did work and then answers is labelled the same way: Answered,
/// never Completed; its work stays unverified.
#[tokio::test]
async fn a_run_that_acted_and_then_answers_is_answered_not_completed() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
    responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("r1"),
                responses::ev_function_call("call-1", "no_such_tool", "{}"),
                responses::ev_completed("r1"),
            ]),
            message("r2", &ready()),
        ],
    )
    .await;
    harness.turn().await?;
    assert_eq!(harness.status(&run.id).await?, StatefulRunStatus::Answered);
    Ok(())
}

/// Anything that is not a genuine, ready final answer keeps the ordinary route: no block,
/// work that continues, open issues, a declaration with no answer, a block that does not end
/// the message, a ready block in commentary, and a Collaborative run.
#[tokio::test]
async fn anything_but_a_genuine_ready_answer_keeps_the_ordinary_route() -> Result<()> {
    let continuing = format!(
        "I will read the parser next.\n\n{}",
        BLOCK.replace("disposition: answer", "disposition: continue")
    );
    let open_issues = format!(
        "It probably returns a Result.\n\n{}",
        BLOCK.replace(
            "open-issues: none",
            "open-issues:\n- I did not read the source."
        )
    );
    let cases = [
        (StatefulWorkflowMode::Autonomous, message("r1", ANSWER)),
        (StatefulWorkflowMode::Autonomous, message("r1", &continuing)),
        (
            StatefulWorkflowMode::Autonomous,
            message("r1", &open_issues),
        ),
        (StatefulWorkflowMode::Autonomous, message("r1", BLOCK)),
        (
            StatefulWorkflowMode::Autonomous,
            message("r1", &format!("{}\nAnything else?", ready())),
        ),
        (StatefulWorkflowMode::Autonomous, commentary("r1", &ready())),
        (StatefulWorkflowMode::Collaborative, message("r1", &ready())),
    ];
    for (mode, response) in cases {
        let mut harness = harness().await?;
        let run = harness.start_run(mode).await?;
        responses::mount_sse_once(&harness.responses_server, response.clone()).await;
        harness.turn().await?;
        let read = harness.read(&run.id).await?;
        assert_ne!(
            read.run.expect("run").status,
            StatefulRunStatus::Answered,
            "{response}"
        );
        assert_eq!(read.host_answer, None, "{response}");
    }
    Ok(())
}

/// An interrupt during inference, or while the finalizer waits for the run store, wins: the
/// turn is interrupted and the run stays Running with no answer.
#[tokio::test]
async fn an_interrupt_before_the_commit_leaves_the_run_running() -> Result<()> {
    for wait_for_finalizer in [false, true] {
        let mut harness = harness().await?;
        let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
        let mock = responses::mount_response_once(
            &harness.responses_server,
            held(message("r1", &ready())),
        )
        .await;
        let turn_id = harness.start_turn().await?;
        tokio::time::timeout(Duration::from_secs(10), async {
            while mock.requests().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        let lock = if wait_for_finalizer {
            let lock = harness.hold_run_store().await?;
            harness
                .server
                .read_stream_until_matching_notification(
                    "agent message completed",
                    |notification| {
                        notification.method == "item/completed"
                            && notification
                                .params
                                .as_ref()
                                .and_then(|params| params.get("item"))
                                .and_then(|item| item.get("type"))
                                .and_then(serde_json::Value::as_str)
                                == Some("agentMessage")
                    },
                )
                .await?;
            Some(lock)
        } else {
            None
        };
        let request_id = harness
            .server
            .send_turn_interrupt_request(TurnInterruptParams {
                thread_id: harness.thread_id.clone(),
                turn_id: turn_id.clone(),
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
        assert_eq!(
            read.run.expect("run").status,
            StatefulRunStatus::Running,
            "finalizer wait: {wait_for_finalizer}"
        );
        assert_eq!(read.host_answer, None);
    }
    Ok(())
}
