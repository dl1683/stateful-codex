//! The no-tool exemption through the public API: a run whose turns made no tool call and only
//! answered in text completes without an acceptance ledger. Any other action (a command, even
//! a read; a call beside the completion in the same response, in either order; an earlier
//! bookkeeping call; a hosted call, even in a response that then failed; a call after the run
//! was admitted mid-response; a rejected earlier completion attempt; a user shell command,
//! even while the thread shows no project; a configured hook) brings E's ledger back, and so
//! does a run this process did not observe from its start. A provider-hosted tool that is
//! offered but not called changes nothing. Records that cannot be written fail closed: the
//! action does not run.
//!
//! Hosted web search is offered by default; the tests disable it except where they show that
//! merely offering it keeps the exemption.

use std::path::Path;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::HooksListParams;
use codex_app_server_protocol::HooksListResponse;
use codex_app_server_protocol::MergeStrategy;
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
use codex_app_server_protocol::ThreadMetadataUpdateParams;
use codex_app_server_protocol::ThreadMetadataUpdateResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadShellCommandParams;
use codex_app_server_protocol::ThreadShellCommandResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::StreamingSseServer;
use core_test_support::streaming_sse::start_streaming_sse_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::oneshot;
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
    project_id: String,
    thread_id: String,
    run: Option<StatefulRun>,
}

struct Setup {
    mode: StatefulWorkflowMode,
    /// `thread/shellCommand` runs on the app-server host, without the test environment.
    host_shell: bool,
    /// Managed hook configuration written before the server starts.
    requirements: Option<String>,
    /// The run is admitted with the harness; otherwise a test admits it with `start_run`.
    run_at_start: bool,
    /// Offer the provider-hosted web search tool, as the default configuration does.
    hosted_web_search: bool,
    /// A gated streaming responses server used in place of the mock server.
    responses_uri: Option<String>,
    /// User hook configuration (untrusted until a test trusts it); enables lifecycle hooks.
    user_hooks: Option<String>,
    /// Model commands may write the workspace (otherwise the sandbox is read-only).
    workspace_write: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            mode: StatefulWorkflowMode::Autonomous,
            host_shell: false,
            requirements: None,
            run_at_start: true,
            hosted_web_search: false,
            responses_uri: None,
            user_hooks: None,
            workspace_write: false,
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
    let mut config = MockResponsesConfig::new(
        setup
            .responses_uri
            .as_deref()
            .unwrap_or(&responses_server.uri()),
    )
    .with_sandbox_mode(if setup.workspace_write {
        "workspace-write"
    } else {
        "read-only"
    })
    .enable_feature(Feature::Sqlite);
    if !setup.hosted_web_search {
        config = config.with_root_config("web_search = \"disabled\"");
    }
    if let Some(user_hooks) = &setup.user_hooks {
        config = config
            .enable_feature(Feature::CodexHooks)
            .with_extra_config(user_hooks);
    }
    if setup.responses_uri.is_some() {
        config = config
            .with_provider_config("supports_websockets = false")
            .disable_feature(Feature::EnableRequestCompression);
    }
    config.write(codex_home.path())?;
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
        // Raw response items show when a call has passed the fence and reached Core.
        experimental_raw_events: true,
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
    let mut harness = Harness {
        codex_home,
        project_root,
        responses_server,
        server,
        project_id: project.project.id,
        thread_id: thread.thread.id,
        run: None,
    };
    if setup.run_at_start {
        harness.start_run(setup.mode).await?;
    }
    Ok(harness)
}

impl Harness {
    fn run(&self) -> &StatefulRun {
        self.run.as_ref().expect("the run was admitted")
    }

    /// Admits the run through the public `statefulRun/start`.
    async fn start_run(&mut self, mode: StatefulWorkflowMode) -> Result<()> {
        let project_id = self.project_id.clone();
        let thread_id = self.thread_id.clone();
        let started: StatefulRunStartResponse = self
            .server
            .request(|request_id| ClientRequest::StatefulRunStart {
                request_id,
                params: StatefulRunStartParams {
                    project_id,
                    thread_id,
                    goal: LOOKUP.to_string(),
                    mode,
                    budget: StatefulRunBudget {
                        max_continuations: 1,
                        max_elapsed_seconds: 3_600,
                    },
                    idempotency_key: "exemption-run".to_string(),
                },
            })
            .await?;
        self.run = Some(started.run);
        Ok(())
    }

    /// Selects `project_id` for the thread through `thread/metadata/update`; empty clears it.
    async fn select_project(&mut self, project_id: &str) -> Result<()> {
        let thread_id = self.thread_id.clone();
        let project_id = project_id.to_string();
        let _: ThreadMetadataUpdateResponse = self
            .server
            .request(|request_id| ClientRequest::ThreadMetadataUpdate {
                request_id,
                params: ThreadMetadataUpdateParams {
                    thread_id,
                    project_id: Some(project_id),
                    git_info: None,
                    daybreak_enabled: None,
                },
            })
            .await?;
        Ok(())
    }

    /// Starts a user turn without waiting for it; returns its id.
    async fn begin_turn(&mut self) -> Result<String> {
        let thread_id = self.thread_id.clone();
        let started: TurnStartResponse = self
            .server
            .request(|request_id| ClientRequest::TurnStart {
                request_id,
                params: TurnStartParams {
                    thread_id,
                    input: vec![UserInput::Text {
                        text: "Answer.".to_string(),
                        text_elements: Vec::new(),
                    }],
                    ..Default::default()
                },
            })
            .await?;
        Ok(started.turn.id)
    }

    async fn wait_for_turn(&mut self, turn_id: &str) -> Result<TurnCompletedNotification> {
        let notification = timeout(
            READ_TIMEOUT,
            self.server.read_stream_until_matching_notification(
                "turn/completed for the started turn",
                |notification| {
                    notification.method == "turn/completed"
                        && notification
                            .params
                            .as_ref()
                            .is_some_and(|params| params["turn"]["id"].as_str() == Some(turn_id))
                },
            ),
        )
        .await??;
        Ok(serde_json::from_value(
            notification.params.unwrap_or_default(),
        )?)
    }

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
        let run_id = self.run().id.clone();
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
        let pool = self.open_runtime_store().await?;
        sqlx::query(sqlx::AssertSqlSafe(statement.to_string()))
            .execute(&pool)
            .await?;
        pool.close().await;
        Ok(())
    }

    /// The run's durable action count, read through a freshly opened store.
    async fn recorded_actions(&self) -> Result<i64> {
        let pool = self.open_runtime_store().await?;
        let actions = sqlx::query_scalar::<_, i64>(
            "SELECT side_effects FROM stateful_acceptance_ledgers WHERE run_id = ?",
        )
        .bind(self.run().id.clone())
        .fetch_one(&pool)
        .await?;
        pool.close().await;
        Ok(actions)
    }

    /// Whether the run's ledger durably records the no-tool exemption, through a fresh store.
    async fn recorded_exemption(&self) -> Result<bool> {
        let pool = self.open_runtime_store().await?;
        let exemption = sqlx::query_scalar::<_, Option<String>>(
            "SELECT exemption FROM stateful_acceptance_ledgers WHERE run_id = ?",
        )
        .bind(self.run().id.clone())
        .fetch_one(&pool)
        .await?;
        pool.close().await;
        Ok(exemption.is_some())
    }

    /// Asserts, through a reopened store, that the run is still running, holds no result or
    /// exemption, and has recorded actions.
    async fn assert_left_running_with_actions(&mut self) -> Result<()> {
        let run = self.read().await?;
        assert_eq!(
            (run.status, run.result.clone()),
            (StatefulRunStatus::Running, None)
        );
        assert!(!self.recorded_exemption().await?);
        assert!(self.recorded_actions().await? > 0);
        Ok(())
    }

    async fn open_runtime_store(&self) -> Result<sqlx::SqlitePool> {
        let sqlite = SqliteConfig::new_for_testing(AbsolutePathBuf::try_from(
            self.codex_home.path().to_path_buf(),
        )?);
        Ok(sqlite
            .open_read_write_pool(&self.codex_home.path().join("stateful_runtime_1.sqlite"))
            .await?)
    }
}

/// Test 1: a pure Q&A run, whose only call is its completion, completes without a ledger and
/// says so in its durable result.
#[tokio::test]
async fn a_text_only_answer_completes_without_a_ledger() -> Result<()> {
    let mut harness = harness(Setup::default()).await?;
    let revision = harness.run().revision;
    let (_, output) = harness
        .turn(vec![complete(revision), message("done", ANSWER)])
        .await?;
    let output = output.unwrap_or_default();
    assert!(output.contains("\"completionPending\":true"), "{output}");
    // The completion commits when its turn ends, which happened before turn/completed.
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
    let revision = harness.run().revision;
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
            completion_arguments(harness.run().revision, &[]),
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
    let revision = harness.run().revision;
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
        error.contains("the host could not durably record a model call before dispatch"),
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
    let revision = harness.run().revision;
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
    let revision = harness.run().revision;
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
    let revision = harness.run().revision;
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
    let revision = harness.run().revision;
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

/// Finds `call_id`'s output in the requests a streaming server received.
async fn streamed_output(server: &StreamingSseServer, call_id: &str) -> Option<String> {
    server.requests().await.iter().find_map(|body| {
        let body: Value = serde_json::from_slice(body).ok()?;
        body["input"].as_array()?.iter().find_map(|item| {
            (item["type"] == "function_call_output" && item["call_id"] == call_id)
                .then(|| item["output"].as_str().map(str::to_string))
                .flatten()
        })
    })
}

fn chunk(gate: Option<oneshot::Receiver<()>>, events: Vec<Value>) -> StreamingSseChunk {
    StreamingSseChunk {
        gate,
        body: responses::sse(events),
    }
}

/// A hosted call is recorded as soon as the host first observes it, added or done, and the
/// record survives the response failing before it completes: after either witness the
/// reopened store shows the action and a later lone completion owes the ledger. Hosted tools
/// are not offered here, so only the observation can end the exemption.
#[tokio::test]
async fn a_hosted_call_in_a_failed_response_ends_the_exemption() -> Result<()> {
    for completion_first in [true, false] {
        let mut harness = harness(Setup::default()).await?;
        let revision = harness.run().revision;
        let mut events = vec![responses::ev_response_created("failed")];
        if completion_first {
            events.push(responses::ev_function_call(
                "early",
                "stateful_run_update",
                &completion_arguments(revision, &[]).to_string(),
            ));
            events.push(responses::ev_web_search_call_done(
                "search",
                "completed",
                "parser",
            ));
        } else {
            events.push(responses::ev_web_search_call_added_partial(
                "search",
                "in_progress",
            ));
        }
        // The response ends without `response.completed`: the stream fails.
        let (status, _) = harness.turn(vec![responses::sse(events)]).await?;
        assert_eq!(
            status,
            TurnStatus::Failed,
            "completion first: {completion_first}"
        );
        assert!(
            harness.recorded_actions().await? > 0,
            "completion first: {completion_first}"
        );
        let (_, output) = harness
            .turn(vec![complete(revision), message("done", "Refused.")])
            .await?;
        harness.assert_refused(output).await?;
    }
    Ok(())
}

/// Offering a provider-hosted tool is not a call: with web search offered, as it is by default,
/// a run that never calls it completes without a ledger.
#[tokio::test]
async fn an_offered_but_uncalled_hosted_tool_keeps_the_exemption() -> Result<()> {
    let mut harness = harness(Setup {
        hosted_web_search: true,
        ..Setup::default()
    })
    .await?;
    let revision = harness.run().revision;
    let log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![complete(revision), message("done", ANSWER)],
    )
    .await;
    let turn_id = harness.begin_turn().await?;
    harness.wait_for_turn(&turn_id).await?;
    let offered = log.requests()[0].body_json()["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == "web_search"));
    assert!(offered, "the request offered hosted web search");
    let output = log
        .function_call_output_text("complete")
        .unwrap_or_default();
    assert!(output.contains("\"completionPending\":true"), "{output}");
    let run = harness.read().await?;
    assert_eq!(run.status, StatefulRunStatus::Completed);
    assert!(
        run.result
            .unwrap_or_default()
            .contains("no-tool exemption: the host recorded no tool call"),
        "the durable result carries the exemption basis"
    );
    Ok(())
}

/// A run admitted through `statefulRun/start` while a response is still streaming is charged
/// for the calls that follow: each call is recorded against the run bindings current when it
/// arrives, never certified by an earlier record of the same response.
#[tokio::test]
async fn a_run_admitted_mid_response_is_charged_for_later_calls() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let (_, _, plan_arguments) = plan();
    let plan_arguments = plan_arguments.to_string();
    let (streaming, _) = start_streaming_sse_server(vec![
        vec![
            chunk(
                None,
                vec![
                    responses::ev_response_created("calls"),
                    responses::ev_function_call("plan", "update_plan", &plan_arguments),
                ],
            ),
            chunk(
                Some(gate),
                vec![
                    responses::ev_function_call("plan-2", "update_plan", &plan_arguments),
                    responses::ev_function_call(
                        "complete",
                        "stateful_run_update",
                        &completion_arguments(/*revision*/ 1, &[]).to_string(),
                    ),
                    responses::ev_completed("calls"),
                ],
            ),
        ],
        vec![chunk(
            None,
            vec![
                responses::ev_assistant_message("done", "Refused."),
                responses::ev_completed("done"),
            ],
        )],
    ])
    .await;
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        run_at_start: false,
        responses_uri: Some(streaming.uri().to_string()),
        ..Setup::default()
    })
    .await?;
    let turn_id = harness.begin_turn().await?;
    // The first call passed the fence and reached Core while no run was bound to the thread.
    timeout(
        READ_TIMEOUT,
        harness.server.read_stream_until_matching_notification(
            "the first call reached Core",
            |notification| {
                notification.method == "rawResponseItem/completed"
                    && notification
                        .params
                        .as_ref()
                        .is_some_and(|params| params["item"]["call_id"] == "plan")
            },
        ),
    )
    .await??;
    harness
        .start_run(StatefulWorkflowMode::Collaborative)
        .await?;
    assert_eq!(harness.run().revision, 1);
    release.send(()).expect("response still streaming");
    harness.wait_for_turn(&turn_id).await?;
    harness
        .assert_refused(streamed_output(&streaming, "complete").await)
        .await?;
    assert!(harness.recorded_actions().await? > 0);
    streaming.shutdown().await;
    Ok(())
}

/// User shell accounting follows the run's durable thread binding, not the thread's current
/// project selection. With the selection cleared, a command whose record fails is refused
/// before it runs; once records work again, an auxiliary command during a model turn is
/// recorded, so restoring the selection and completing alone is not exempt.
#[tokio::test]
async fn a_user_shell_command_counts_while_the_thread_shows_no_project() -> Result<()> {
    let (release, gate) = oneshot::channel();
    let revision = 1;
    let (streaming, _) = start_streaming_sse_server(vec![
        vec![chunk(
            None,
            vec![
                responses::ev_assistant_message("first", "Looking at it."),
                responses::ev_completed("first"),
            ],
        )],
        vec![chunk(
            Some(gate),
            vec![
                responses::ev_response_created("lone"),
                responses::ev_function_call(
                    "complete",
                    "stateful_run_update",
                    &completion_arguments(revision, &[]).to_string(),
                ),
                responses::ev_completed("lone"),
            ],
        )],
        vec![chunk(
            None,
            vec![
                responses::ev_assistant_message("done", "Refused."),
                responses::ev_completed("done"),
            ],
        )],
    ])
    .await;
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        host_shell: true,
        responses_uri: Some(streaming.uri().to_string()),
        ..Setup::default()
    })
    .await?;
    assert_eq!(harness.run().revision, revision);
    let project_id = harness.project_id.clone();
    // A text-only turn gives the thread its stored metadata; it records no action.
    let turn_id = harness.begin_turn().await?;
    harness.wait_for_turn(&turn_id).await?;

    harness.break_action_records().await?;
    harness.select_project("").await?;
    let refused_marker = harness.project_root.path().join("refused.txt");
    let refusal = harness
        .refused_shell(write_command(&refused_marker))
        .await?;
    assert!(
        refusal.starts_with("the shell command was not run: the host could not durably record it"),
        "{refusal}"
    );
    assert!(!refused_marker.exists(), "an unrecorded command never runs");
    harness.heal().await?;
    assert_eq!(harness.recorded_actions().await?, 0);

    harness.select_project(&project_id).await?;
    let turn_id = harness.begin_turn().await?;
    timeout(READ_TIMEOUT, streaming.wait_for_request_count(2)).await?;
    harness.select_project("").await?;
    let marker = harness.project_root.path().join("auxiliary.txt");
    let thread_id = harness.thread_id.clone();
    let command = write_command(&marker);
    let _: ThreadShellCommandResponse = harness
        .server
        .request(|request_id| ClientRequest::ThreadShellCommand {
            request_id,
            params: ThreadShellCommandParams {
                thread_id,
                command,
                timeout_ms: None,
            },
        })
        .await?;
    timeout(READ_TIMEOUT, async {
        while harness.recorded_actions().await? == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    // The Windows gate host cannot spawn its default shell (`Access is denied`); the action is
    // recorded before the spawn either way.
    if cfg!(not(target_os = "windows")) {
        timeout(READ_TIMEOUT, async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await?;
    }
    harness.select_project(&project_id).await?;
    release.send(()).expect("response still streaming");
    harness.wait_for_turn(&turn_id).await?;
    harness
        .assert_refused(streamed_output(&streaming, "complete").await)
        .await?;
    streaming.shutdown().await;
    Ok(())
}

/// Only the run's own successful completion is exempt: a completion attempt the tool rejected
/// (missing openIssues, or malformed completion-shaped arguments) was a call, so the corrected
/// completion that follows owes the ledger.
#[tokio::test]
async fn a_rejected_completion_attempt_brings_the_ledger_back() -> Result<()> {
    for rejected in [
        json!({"status": "completed", "completionDisposition": "noReusableLearning", "result": ANSWER}),
        json!({"status": "completed", "openIssues": "none", "result": ANSWER}),
    ] {
        let mut harness = harness(Setup::default()).await?;
        let revision = harness.run().revision;
        let mut rejected = rejected;
        rejected["expectedRevision"] = json!(revision);
        let log = responses::mount_sse_sequence(
            &harness.responses_server,
            vec![
                call("attempt", "stateful_run_update", rejected.clone()),
                complete(revision),
                message("done", "Refused."),
            ],
        )
        .await;
        let turn_id = harness.begin_turn().await?;
        harness.wait_for_turn(&turn_id).await?;
        // The tool rejected the attempt; the run kept running.
        let attempt = log.function_call_output_text("attempt").unwrap_or_default();
        assert!(
            !attempt.is_empty() && !attempt.contains("no-tool exemption"),
            "{rejected}: {attempt}"
        );
        harness
            .assert_refused(log.function_call_output_text("complete"))
            .await?;
    }
    Ok(())
}

/// A hook trusted while an exempt completion is pending cannot run around that completion or
/// later in its turn: a turn's hooks are fixed when it starts, and publishing hooks records the
/// host's hook fact first. The order is the confirmation review's: an untrusted PostToolUse and
/// Stop hook, a lone completion waiting inside its acceptance decision (for a command held
/// open), the hooks trusted through `config/batchWrite` with a user-config reload, then the
/// completion released. No hook starts in that turn, whatever the completion's outcome; the
/// next turn runs the now-trusted Stop hook, so the hooks were live.
#[tokio::test]
async fn a_hook_trusted_during_a_pending_completion_cannot_run_around_it() -> Result<()> {
    let hook_home = TempDir::new()?;
    let post_marker = hook_home.path().join("post.txt");
    let stop_marker = hook_home.path().join("stop.txt");
    let user_hooks = format!(
        "[hooks]\n\n[[hooks.PostToolUse]]\n\n[[hooks.PostToolUse.hooks]]\ntype = 'command'\ncommand = 'echo hooked > \"{}\"'\n\n[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\ntype = 'command'\ncommand = 'echo stopped > \"{}\"'\n",
        post_marker.display(),
        stop_marker.display()
    );
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        user_hooks: Some(user_hooks),
        ..Setup::default()
    })
    .await?;
    let revision = harness.run().revision;
    // A text-only first turn binds the run to this process (its first binding clears pending
    // commands); it records no action.
    harness
        .turn(vec![message("first", "Looking at it.")])
        .await?;
    // A started command keeps the completion waiting inside its acceptance decision, after it
    // read the hook fact, until the test settles the command.
    harness
        .ledger_sql(&format!(
            "INSERT INTO stateful_acceptance_pending (run_id, call_id, started_at_ms) VALUES ('{}', 'held-command', 0)",
            harness.run().id
        ))
        .await?;
    let log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![complete(revision), message("done", ANSWER)],
    )
    .await;
    let turn_id = harness.begin_turn().await?;
    timeout(READ_TIMEOUT, async {
        while log.requests().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await?;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let cwd = harness.project_root.path().to_path_buf();
    let listed: HooksListResponse = harness
        .server
        .request(|request_id| ClientRequest::HooksList {
            request_id,
            params: HooksListParams { cwds: vec![cwd] },
        })
        .await?;
    let trusted = listed
        .data
        .iter()
        .flat_map(|entry| entry.hooks.iter())
        .map(|hook| (hook.key.clone(), json!({"trusted_hash": hook.current_hash})))
        .collect::<serde_json::Map<String, Value>>();
    assert_eq!(trusted.len(), 2, "both hooks are listed");
    let _: ConfigWriteResponse = harness
        .server
        .request(|request_id| ClientRequest::ConfigBatchWrite {
            request_id,
            params: ConfigBatchWriteParams {
                edits: vec![ConfigEdit {
                    key_path: "hooks.state".to_string(),
                    value: Value::Object(trusted),
                    merge_strategy: MergeStrategy::Upsert,
                }],
                file_path: None,
                expected_version: None,
                reload_user_config: true,
            },
        })
        .await?;
    harness
        .ledger_sql("DELETE FROM stateful_acceptance_pending")
        .await?;
    harness.wait_for_turn(&turn_id).await?;

    assert!(
        !harness
            .server
            .pending_notification_methods()
            .iter()
            .any(|method| method == "hook/started"),
        "no hook started in the completion's turn"
    );
    assert!(!post_marker.exists() && !stop_marker.exists());
    // Whatever the completion read, the run is truthful: exempt with no hook run, or refused.
    let output = log
        .function_call_output_text("complete")
        .unwrap_or_default();
    let run = harness.read().await?;
    let exempt = run.status == StatefulRunStatus::Completed
        && run
            .result
            .as_deref()
            .is_some_and(|result| result.contains("no-tool exemption"));
    let not_exempt = run.status == StatefulRunStatus::Running && run.result.is_none();
    assert!(exempt || not_exempt, "{run:?}: {output}");

    // The trusted hooks are live from the next turn on.
    harness.server.clear_message_buffer();
    harness.turn(vec![message("later", "Later.")]).await?;
    assert!(
        harness
            .server
            .pending_notification_methods()
            .iter()
            .any(|method| method == "hook/started"),
        "the Stop hook runs in the next turn"
    );
    if cfg!(not(target_os = "windows")) {
        assert!(stop_marker.exists(), "the Stop hook ran in the next turn");
    }
    Ok(())
}

/// A response whose hold overflows by count or by bytes on a hosted call (added or done) fails
/// without releasing its completion, and the hosted call is still recorded first: a reopened
/// store shows the run is no longer action-free, and a later lone completion owes the ledger.
#[tokio::test]
async fn a_hosted_call_that_overflows_the_hold_still_ends_the_exemption() -> Result<()> {
    const HOLD_EVENTS: usize = 1_024;
    const HOLD_BYTES: usize = 1024 * 1024;
    for (by_count, added) in [(true, true), (true, false), (false, true), (false, false)] {
        let mut harness = harness(Setup {
            mode: StatefulWorkflowMode::Collaborative,
            ..Setup::default()
        })
        .await?;
        let revision = harness.run().revision;
        let mut events = vec![
            responses::ev_response_created("overflow"),
            responses::ev_function_call(
                "early",
                "stateful_run_update",
                &completion_arguments(revision, &[]).to_string(),
            ),
        ];
        let long = "q".repeat(6_000);
        if by_count {
            events.extend((1..HOLD_EVENTS).map(|_| responses::ev_output_text_delta("")));
        } else {
            events.push(responses::ev_output_text_delta(
                &"x".repeat(HOLD_BYTES - 5_000),
            ));
        }
        let hosted_id = if by_count { "search" } else { long.as_str() };
        events.push(if added {
            responses::ev_web_search_call_added_partial(hosted_id, "in_progress")
        } else {
            responses::ev_web_search_call_done(hosted_id, "completed", &long)
        });
        let case = format!("by count {by_count}, added {added}");
        let log =
            responses::mount_sse_sequence(&harness.responses_server, vec![responses::sse(events)])
                .await;
        let turn_id = harness.begin_turn().await?;
        let completed = harness.wait_for_turn(&turn_id).await?;
        assert_eq!(completed.turn.status, TurnStatus::Failed, "{case}");
        // The held completion never ran: no follow-up request carried its output.
        assert_eq!(log.requests().len(), 1, "{case}");
        assert!(harness.recorded_actions().await? > 0, "{case}");
        let (_, output) = harness
            .turn(vec![complete(revision), message("done", "Refused.")])
            .await?;
        harness.assert_refused(output).await?;
    }
    Ok(())
}

/// A call later in the completing turn, after the lone completion, keeps the run from
/// completing under the exemption: the completion only became pending, the later call was
/// recorded against the still-open run and actually ran, and when the turn ended the run
/// stayed running without an exemption.
#[tokio::test]
async fn a_call_after_the_completion_in_its_turn_ends_the_exemption() -> Result<()> {
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        ..Setup::default()
    })
    .await?;
    let revision = harness.run().revision;
    let log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            complete(revision),
            call("read", "stateful_run_read", json!({"section": "goal"})),
            message("done", ANSWER),
        ],
    )
    .await;
    let turn_id = harness.begin_turn().await?;
    harness.wait_for_turn(&turn_id).await?;
    let pending = log
        .function_call_output_text("complete")
        .unwrap_or_default();
    assert!(pending.contains("\"completionPending\":true"), "{pending}");
    let read = log.function_call_output_text("read").unwrap_or_default();
    assert!(read.contains(LOOKUP), "the later call ran: {read}");
    harness.assert_left_running_with_actions().await
}

/// The same with a shell command after the completion: it is recorded before it runs (on
/// Linux it writes its marker), and the run stays running.
#[tokio::test]
async fn a_command_after_the_completion_in_its_turn_ends_the_exemption() -> Result<()> {
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        workspace_write: true,
        ..Setup::default()
    })
    .await?;
    let revision = harness.run().revision;
    let marker = harness.project_root.path().join("after.txt");
    let root = harness.project_root.path().to_string_lossy().to_string();
    let log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            complete(revision),
            call(
                "write",
                "exec_command",
                json!({"cmd": write_command(&marker), "workdir": root, "yield_time_ms": 10_000}),
            ),
            message("done", ANSWER),
        ],
    )
    .await;
    let turn_id = harness.begin_turn().await?;
    harness.wait_for_turn(&turn_id).await?;
    // The later command was dispatched: its output went back to the model.
    assert!(log.function_call_output_text("write").is_some());
    if cfg!(not(target_os = "windows")) {
        assert!(marker.exists(), "the later command ran");
    }
    harness.assert_left_running_with_actions().await
}

/// A hosted call observed in a later response of the completing turn is recorded, and the run
/// stays running.
#[tokio::test]
async fn a_hosted_call_after_the_completion_in_its_turn_ends_the_exemption() -> Result<()> {
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        ..Setup::default()
    })
    .await?;
    let revision = harness.run().revision;
    harness
        .turn(vec![
            complete(revision),
            responses::sse(vec![
                responses::ev_response_created("searched"),
                responses::ev_web_search_call_done("search", "completed", "parser"),
                responses::ev_assistant_message("done", ANSWER),
                responses::ev_completed("searched"),
            ]),
        ])
        .await?;
    harness.assert_left_running_with_actions().await
}

/// A lone completion whose result does not fit in its response is not exempt (reading it back
/// would be a further call): it is refused and the run stays running.
#[tokio::test]
async fn an_oversized_result_is_not_exempt() -> Result<()> {
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        ..Setup::default()
    })
    .await?;
    let revision = harness.run().revision;
    let mut arguments = completion_arguments(revision, &[]);
    arguments["result"] = json!("x".repeat(30_000));
    let (_, output) = harness
        .turn(vec![
            call("complete", "stateful_run_update", arguments),
            message("done", "Refused."),
        ])
        .await?;
    let output = output.unwrap_or_default();
    assert!(output.contains("does not fit in this response"), "{output}");
    harness.assert_left_running_with_actions().await
}

/// An interrupted turn never commits the completion it left pending.
#[tokio::test]
async fn an_interrupted_completing_turn_is_not_exempt() -> Result<()> {
    let (_never, gate) = oneshot::channel();
    let (streaming, _) = start_streaming_sse_server(vec![
        vec![chunk(
            None,
            vec![
                responses::ev_response_created("lone"),
                responses::ev_function_call(
                    "complete",
                    "stateful_run_update",
                    &completion_arguments(/*revision*/ 1, &[]).to_string(),
                ),
                responses::ev_completed("lone"),
            ],
        )],
        vec![chunk(
            Some(gate),
            vec![
                responses::ev_assistant_message("done", ANSWER),
                responses::ev_completed("done"),
            ],
        )],
    ])
    .await;
    let mut harness = harness(Setup {
        mode: StatefulWorkflowMode::Collaborative,
        responses_uri: Some(streaming.uri().to_string()),
        ..Setup::default()
    })
    .await?;
    assert_eq!(harness.run().revision, 1);
    let turn_id = harness.begin_turn().await?;
    // The completion ran and the turn is waiting for the final answer.
    timeout(READ_TIMEOUT, streaming.wait_for_request_count(2)).await?;
    let thread_id = harness.thread_id.clone();
    let interrupted_turn = turn_id.clone();
    let _: TurnInterruptResponse = harness
        .server
        .request(|request_id| ClientRequest::TurnInterrupt {
            request_id,
            params: TurnInterruptParams {
                thread_id,
                turn_id: interrupted_turn,
            },
        })
        .await?;
    harness.wait_for_turn(&turn_id).await?;
    assert!(
        streamed_output(&streaming, "complete")
            .await
            .unwrap_or_default()
            .contains("\"completionPending\":true")
    );
    let run = harness.read().await?;
    assert_eq!((run.status, run.result), (StatefulRunStatus::Running, None));
    assert!(!harness.recorded_exemption().await?);
    streaming.shutdown().await;
    Ok(())
}
