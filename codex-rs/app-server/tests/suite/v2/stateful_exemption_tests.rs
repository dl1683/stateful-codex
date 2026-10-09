//! The no-tool exemption through the public API: a run whose turns made no tool call and only
//! answered in text completes without an acceptance ledger. Any other action (a command, even
//! a read; a call beside the completion in the same response, in either order; an earlier
//! bookkeeping call; a user shell command; a configured hook) brings E's ledger back, and so
//! does a run this process did not observe from its start. Records that cannot be written fail
//! closed: the action does not run.

use std::path::Path;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadShellCommandParams;
use codex_app_server_protocol::ThreadShellCommandResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const LOOKUP: &str = "What does the parser module do?";
const ANSWER: &str = "It turns tokens into an AST.";
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

fn response(id: &str, calls: &[(&str, &str, Value)]) -> String {
    let mut events = vec![responses::ev_response_created(id)];
    events.extend(calls.iter().map(|(call_id, tool, arguments)| {
        responses::ev_function_call(call_id, tool, &arguments.to_string())
    }));
    events.push(responses::ev_completed(id));
    responses::sse(events)
}

fn call(call_id: &str, tool: &str, arguments: Value) -> String {
    response(call_id, &[(call_id, tool, arguments)])
}

fn completion_arguments(revision: u64, open_issues: &[&str]) -> Value {
    json!({
        "expectedRevision": revision,
        "status": "completed",
        "openIssues": open_issues,
        "completionDisposition": "noReusableLearning",
        "result": ANSWER,
    })
}

fn complete(revision: u64) -> String {
    call(
        "complete",
        "stateful_run_update",
        completion_arguments(revision, &[]),
    )
}

fn plan() -> (&'static str, &'static str, Value) {
    (
        "plan",
        "update_plan",
        json!({"plan": [{"step": "Answer from the README", "status": "completed"}]}),
    )
}

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

/// A shell command that writes `marker`, valid for the platform's default shell.
fn write_command(marker: &Path) -> String {
    format!("echo written > \"{}\"", marker.display())
}

struct Harness {
    codex_home: TempDir,
    project_root: TempDir,
    responses_server: wiremock::MockServer,
    server: TestAppServer,
    thread_id: String,
    run: StatefulRun,
}

struct Setup {
    mode: StatefulWorkflowMode,
    /// `thread/shellCommand` runs on the app-server host, without the test environment.
    host_shell: bool,
    /// Managed hook configuration written before the server starts.
    requirements: Option<String>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            mode: StatefulWorkflowMode::Autonomous,
            host_shell: false,
            requirements: None,
        }
    }
}

async fn start_server(codex_home: &Path, host_shell: bool) -> Result<TestAppServer> {
    let builder = TestAppServer::builder().with_codex_home(codex_home);
    let builder = if host_shell {
        builder.without_auto_env()
    } else {
        builder
    };
    builder.build_initialized().await
}

async fn harness(setup: Setup) -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("README.md"),
        "The parser module turns tokens into an AST.\n",
    )?;
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode("read-only")
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    if let Some(requirements) = &setup.requirements {
        std::fs::write(codex_home.path().join("requirements.toml"), requirements)?;
    }
    let mut server = start_server(codex_home.path(), setup.host_shell).await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "No-tool exemption".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "exemption-project".to_string(),
            },
        })
        .await?;
    let thread_params = ThreadStartParams {
        project_id: Some(project.project.id.clone()),
        cwd: Some(project_root.path().to_string_lossy().to_string()),
        ..Default::default()
    };
    let thread: ThreadStartResponse = if setup.host_shell {
        server
            .request(|request_id| ClientRequest::ThreadStart {
                request_id,
                params: thread_params,
            })
            .await?
    } else {
        server.start_thread(thread_params).await?
    };
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: LOOKUP.to_string(),
                mode: setup.mode,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "exemption-run".to_string(),
            },
        })
        .await?;
    Ok(Harness {
        codex_home,
        project_root,
        responses_server,
        server,
        thread_id: thread.thread.id,
        run: started.run,
    })
}

impl Harness {
    /// Runs one user turn against `bodies`; returns the turn status and the completion output.
    async fn turn(&mut self, bodies: Vec<String>) -> Result<(TurnStatus, Option<String>)> {
        let log = responses::mount_sse_sequence(&self.responses_server, bodies).await;
        let completed = self
            .server
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: self.thread_id.clone(),
                input: vec![UserInput::Text {
                    text: "Answer.".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        Ok((
            completed.turn.status,
            log.requests()
                .iter()
                .find_map(|request| request.function_call_output_text("complete")),
        ))
    }

    async fn read(&mut self) -> Result<StatefulRun> {
        let run_id = self.run.id.clone();
        let read: StatefulRunReadResponse = self
            .server
            .request(|request_id| ClientRequest::StatefulRunRead {
                request_id,
                params: StatefulRunReadParams {
                    run_id: Some(run_id),
                    thread_id: None,
                },
            })
            .await?;
        Ok(read.run.expect("run remains readable"))
    }

    /// Asserts the lone completion `outputs` was refused and the run kept running.
    async fn assert_refused(&mut self, output: Option<String>) -> Result<()> {
        let output = output.unwrap_or_default();
        assert!(output.contains("completion refused"), "{output}");
        assert_eq!(self.read().await?.status, StatefulRunStatus::Running);
        Ok(())
    }

    /// Runs a user shell command whose action record fails; returns the refusal message.
    async fn refused_shell(&mut self, command: String) -> Result<String> {
        let _: ThreadShellCommandResponse = self
            .server
            .request(|request_id| ClientRequest::ThreadShellCommand {
                request_id,
                params: ThreadShellCommandParams {
                    thread_id: self.thread_id.clone(),
                    command,
                    timeout_ms: None,
                },
            })
            .await?;
        let error = timeout(
            READ_TIMEOUT,
            self.server.read_stream_until_notification_message("error"),
        )
        .await??;
        timeout(
            READ_TIMEOUT,
            self.server
                .read_stream_until_notification_message("turn/completed"),
        )
        .await??;
        Ok(error
            .params
            .and_then(|params| params["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_default())
    }

    async fn shell(&mut self, command: String) -> Result<()> {
        let _: ThreadShellCommandResponse = self
            .server
            .request(|request_id| ClientRequest::ThreadShellCommand {
                request_id,
                params: ThreadShellCommandParams {
                    thread_id: self.thread_id.clone(),
                    command,
                    timeout_ms: None,
                },
            })
            .await?;
        timeout(
            READ_TIMEOUT,
            self.server
                .read_stream_until_notification_message("turn/completed"),
        )
        .await??;
        Ok(())
    }

    /// Fault injection: every write of the run's action record fails until `heal`.
    async fn break_action_records(&self) -> Result<()> {
        self.ledger_sql(
            "CREATE TRIGGER fail_action_records BEFORE UPDATE OF side_effects
             ON stateful_acceptance_ledgers
             BEGIN SELECT RAISE(ABORT, 'injected action record failure'); END",
        )
        .await
    }

    async fn heal(&self) -> Result<()> {
        self.ledger_sql("DROP TRIGGER fail_action_records").await
    }

    async fn ledger_sql(&self, statement: &str) -> Result<()> {
        let sqlite = SqliteConfig::new_for_testing(AbsolutePathBuf::try_from(
            self.codex_home.path().to_path_buf(),
        )?);
        let pool = sqlite
            .open_read_write_pool(&self.codex_home.path().join("stateful_runtime_1.sqlite"))
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(statement.to_string()))
            .execute(&pool)
            .await?;
        pool.close().await;
        Ok(())
    }
}

/// Test 1: a pure Q&A run, whose only call is its completion, completes without a ledger and
/// says so in its durable result.
#[tokio::test]
async fn a_text_only_answer_completes_without_a_ledger() -> Result<()> {
    let mut harness = harness(Setup::default()).await?;
    let revision = harness.run.revision;
    let (_, output) = harness
        .turn(vec![complete(revision), message("done", ANSWER)])
        .await?;
    let output = output.unwrap_or_default();
    assert!(output.contains("\"status\":\"completed\""), "{output}");
    let run = harness.read().await?;
    assert_eq!(run.status, StatefulRunStatus::Completed);
    let result = run.result.unwrap_or_default();
    assert!(
        result.starts_with(ANSWER)
            && result.contains("no-tool exemption: the host recorded no tool call"),
        "{result}"
    );
    Ok(())
}

/// Test 2: one read-only command is a tool call, so completion goes through the ledger. The
/// call is recorded before dispatch, so this holds whatever the command does on the platform.
#[tokio::test]
async fn a_read_only_command_brings_the_ledger_back() -> Result<()> {
    let mut harness = harness(Setup::default()).await?;
    let root = harness.project_root.path().to_string_lossy().to_string();
    let revision = harness.run.revision;
    let (_, output) = harness
        .turn(vec![
            call(
                "read",
                "exec_command",
                json!({"cmd": "cat README.md", "workdir": root, "yield_time_ms": 10_000}),
            ),
            complete(revision),
            message("done", "Refused."),
        ])
        .await?;
    harness.assert_refused(output).await
}

/// Test 3: a completion emitted in the same model response as another call is not exempt,
/// whichever comes first.
#[tokio::test]
async fn a_completion_beside_another_call_is_not_exempt() -> Result<()> {
    for completion_first in [true, false] {
        let mut harness = harness(Setup::default()).await?;
        let completion = (
            "complete",
            "stateful_run_update",
            completion_arguments(harness.run.revision, &[]),
        );
        let calls = if completion_first {
            vec![completion, plan()]
        } else {
            vec![plan(), completion]
        };
        let (_, output) = harness
            .turn(vec![response("both", &calls), message("done", "Refused.")])
            .await?;
        harness.assert_refused(output).await?;
    }
    Ok(())
}

/// Test 4: a non-terminal Stateful bookkeeping call earlier in the run ends the exemption.
#[tokio::test]
async fn an_earlier_bookkeeping_call_is_not_exempt() -> Result<()> {
    let mut harness = harness(Setup::default()).await?;
    let revision = harness.run.revision;
    let (_, output) = harness
        .turn(vec![
            call(
                "strategy",
                "stateful_run_update",
                json!({"expectedRevision": revision, "status": "running", "strategy": "Read the README."}),
            ),
            complete(revision + 1),
            message("done", "Refused."),
        ])
        .await?;
    harness.assert_refused(output).await
}

/// Test 5a: when the action record cannot be written, the model's call is never dispatched:
/// the response fails instead, so no follow-up request carries its output.
#[tokio::test]
async fn an_unwritable_action_record_fails_closed() -> Result<()> {
    let mut harness = harness(Setup::default()).await?;
    let marker = harness.project_root.path().join("written.txt");
    let root = harness.project_root.path().to_string_lossy().to_string();
    harness.break_action_records().await?;
    let log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![call(
            "write",
            "exec_command",
            json!({"cmd": write_command(&marker), "workdir": root, "yield_time_ms": 10_000}),
        )],
    )
    .await;
    let completed = harness
        .server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: harness.thread_id.clone(),
            input: vec![UserInput::Text {
                text: "Answer.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    assert_eq!(completed.turn.status, TurnStatus::Failed);
    let error = completed
        .turn
        .error
        .map(|error| error.message)
        .unwrap_or_default();
    assert!(
        error.contains(
            "the host could not durably record this response's tool calls before running them"
        ),
        "{error}"
    );
    // The failure replaced the call: Core never ran it, so no follow-up carried its output.
    assert_eq!(log.requests().len(), 1);
    assert!(!marker.exists());
    harness.heal().await?;
    assert_eq!(harness.read().await?.status, StatefulRunStatus::Running);
    Ok(())
}

/// Test 5b: a run this process did not create (here: created before a restart) has history
/// the host cannot vouch for, so a lone completion owes the ledger.
#[tokio::test]
async fn a_run_resumed_after_a_restart_is_not_exempt() -> Result<()> {
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        ..Setup::default()
    })
    .await?;
    harness
        .turn(vec![message("first", "Looking at it.")])
        .await?;
    harness.server = start_server(harness.codex_home.path(), /*host_shell*/ false).await?;
    let thread_id = harness.thread_id.clone();
    let _: ThreadResumeResponse = harness
        .server
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id,
                ..Default::default()
            },
        })
        .await?;
    let revision = harness.run.revision;
    let (_, output) = harness
        .turn(vec![complete(revision), message("done", "Refused.")])
        .await?;
    harness.assert_refused(output).await
}

/// Test 6a: a user shell command during the run is recorded before it is spawned; with the
/// record broken it is refused and never runs.
#[tokio::test]
async fn a_user_shell_command_ends_the_exemption_and_fails_closed() -> Result<()> {
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        host_shell: true,
        ..Setup::default()
    })
    .await?;
    let refused_marker = harness.project_root.path().join("refused.txt");
    harness.break_action_records().await?;
    let refusal = harness
        .refused_shell(write_command(&refused_marker))
        .await?;
    harness.heal().await?;
    assert!(
        refusal.starts_with("the shell command was not run: the host could not durably record it"),
        "{refusal}"
    );
    assert!(!refused_marker.exists(), "an unrecorded command never runs");

    let marker = harness.project_root.path().join("shell.txt");
    harness.shell(write_command(&marker)).await?;
    // The Windows gate host cannot spawn its default shell (`Access is denied`); the action is
    // recorded before the spawn either way.
    if cfg!(not(target_os = "windows")) {
        assert!(marker.exists(), "the recorded command ran");
    }
    let revision = harness.run.revision;
    let (_, output) = harness
        .turn(vec![complete(revision), message("done", "Refused.")])
        .await?;
    harness.assert_refused(output).await
}

/// Test 6b: a configured command hook could run around the run's turns and calls (here it
/// runs at prompt submission), so the lone completion owes the ledger.
#[tokio::test]
async fn a_configured_command_hook_ends_the_exemption() -> Result<()> {
    let hook_home = TempDir::new()?;
    let marker = hook_home.path().join("hooked.txt");
    let requirements = format!(
        "[hooks]\n\n[[hooks.UserPromptSubmit]]\n\n[[hooks.UserPromptSubmit.hooks]]\ntype = \"command\"\ncommand = 'echo hooked > \"{}\"'\n",
        marker.display()
    );
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        requirements: Some(requirements),
        ..Setup::default()
    })
    .await?;
    let revision = harness.run.revision;
    let (_, output) = harness
        .turn(vec![complete(revision), message("done", "Refused.")])
        .await?;
    // Hook commands cannot be spawned on the Windows gate host (`Access is denied`); the
    // refusal does not depend on the hook having succeeded.
    if cfg!(not(target_os = "windows")) {
        assert!(marker.exists(), "the hook ran during the run");
    }
    harness.assert_refused(output).await
}

/// Test 7: admitted open issues on a zero-tool run still end it Blocked with a partial result.
#[tokio::test]
async fn open_issues_still_block_a_no_tool_run() -> Result<()> {
    let mut harness = harness(Setup::default()).await?;
    let revision = harness.run.revision;
    harness
        .turn(vec![
            call(
                "complete",
                "stateful_run_update",
                completion_arguments(revision, &["The vendor has not confirmed the API."]),
            ),
            message("done", "Blocked."),
        ])
        .await?;
    let run = harness.read().await?;
    assert_eq!(run.status, StatefulRunStatus::Blocked);
    let result = run.result.unwrap_or_default();
    assert!(
        result.starts_with("Partial result: the completion declared unresolved issues"),
        "{result}"
    );
    assert!(
        result.contains("- The vendor has not confirmed the API."),
        "{result}"
    );
    assert!(
        result.contains(&format!(
            "Submitted result (not accepted as complete): {ANSWER}"
        )),
        "{result}"
    );
    Ok(())
}
