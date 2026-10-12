//! "Answered, unverified" through the public API: an Autonomous turn whose final answer ends
//! with the ready outcome block ends its run as Answered (never Completed), with the exact
//! answer and an explicit unverified basis; anything that is not a genuine, ready final
//! answer keeps the ordinary route.
//!
//! The same harness witnesses the deliverable diagnostic: a status-only final reply bound to
//! the turn's run warns once, never retries and never changes the run, the turn status or the
//! committed outcome; it quotes this turn's recorded result only while the run is unchanged.

use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCNotification;
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
use codex_app_server_protocol::WarningNotification;
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
    project_root: TempDir,
    responses_server: wiremock::MockServer,
    server: TestAppServer,
    project_id: String,
    thread_id: String,
}

async fn harness() -> Result<Harness> {
    harness_in("read-only").await
}

/// A harness under `sandbox_mode`; a completion's acceptance check runs a shell command.
async fn harness_in(sandbox_mode: &str) -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::write_acceptance_files(project_root.path())?;
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode(sandbox_mode)
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
        project_root,
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

const STATUS_ONLY: &str =
    "This final reply may omit the deliverable: it contains only a status line.";
const STATUS_LINE: &str = "Task completed.";
const FALLBACK_METADATA_WARNING: &str = "Model metadata for `mock-model` not found.";
/// Runs the acceptance check unsandboxed, so the commit routes are witnessed on every OS.
const CHECK_SANDBOX: &str = "danger-full-access";

fn tool_call(call_id: &str, tool: &str, arguments: serde_json::Value) -> String {
    responses::sse(vec![
        responses::ev_response_created(call_id),
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(call_id),
    ])
}

fn complete(call_id: &str, revision: u64, open_issues: &[&str]) -> String {
    tool_call(
        call_id,
        "stateful_run_update",
        serde_json::json!({
            "expectedRevision": revision,
            "status": "completed",
            "openIssues": open_issues,
            "completionDisposition": "noReusableLearning",
            "result": ANSWER,
        }),
    )
}

fn block(call_id: &str, revision: u64) -> String {
    tool_call(
        call_id,
        "stateful_run_update",
        serde_json::json!({
            "expectedRevision": revision,
            "status": "blocked",
            "result": "Blocked: the parser source is missing.",
        }),
    )
}

/// The match variant, derived from the durable run by the frozen budget rule.
fn matched_warning(reply: &str, run: &StatefulRun) -> String {
    let status = serde_json::to_value(run.status).expect("status serializes");
    let prefix = format!(
        "This final reply may omit the deliverable: it contains only a status line (\"{reply}\"). This turn's recorded result ({}, revision {}): ",
        status.as_str().expect("status name"),
        run.revision
    );
    let result = run.result.clone().unwrap_or_default();
    if prefix.len() + result.len() <= 1_200 {
        return format!("{prefix}{result}");
    }
    let total = result.len();
    let reserved = format!(" [shortened at {total} of {total} bytes]").len();
    let cut = result.floor_char_boundary(1_200 - prefix.len() - reserved);
    format!(
        "{prefix}{} [shortened at {cut} of {total} bytes]",
        &result[..cut]
    )
}

/// Every notification of one turn, up to and including its `turn/completed`.
struct TurnNotifications {
    completed: TurnCompletedNotification,
    notifications: Vec<JSONRPCNotification>,
}

impl TurnNotifications {
    fn warnings(&self) -> Vec<String> {
        self.notifications
            .iter()
            .filter(|notification| notification.method == "warning")
            .map(|notification| {
                serde_json::from_value::<WarningNotification>(
                    notification.params.clone().expect("warning params"),
                )
                .expect("warning notification")
                .message
            })
            // The mock model's fallback-metadata notice is per turn and unrelated.
            .filter(|message| !message.starts_with(FALLBACK_METADATA_WARNING))
            .collect()
    }

    /// The revision of the last run update this turn emitted, if any.
    fn last_run_update_revision(&self) -> Option<u64> {
        self.notifications
            .iter()
            .filter(|notification| notification.method == "statefulRun/updated")
            .filter_map(|notification| notification.params.as_ref()?.get("revision")?.as_u64())
            .last()
    }
}

impl Harness {
    async fn seed_plan(&self, run_id: &str) -> Result<()> {
        super::stateful_acceptance_support::seed_admitted_plan(self.codex_home.path(), run_id, &[])
            .await
    }

    async fn store(&self) -> Result<codex_stateful_runtime::StatefulRunStore> {
        Ok(codex_stateful_runtime::StatefulRunStore::open(&SqliteConfig::new_for_testing(
            self.codex_home.path().abs(),
        ))
        .await?)
    }

    /// A second writer moves the run to `status` with a new result.
    async fn write_run_elsewhere(
        &self,
        run_id: &str,
        status: codex_stateful_runtime::StatefulRunStatus,
    ) -> Result<()> {
        let store = self.store().await?;
        let run_id = codex_stateful_runtime::StatefulRunId::parse(run_id)?;
        let current = store.get_run(&run_id).await?.expect("run");
        store
            .update_run(
                &run_id,
                codex_stateful_runtime::StatefulRunUpdate {
                    expected_revision: current.revision,
                    status,
                    strategy: current.strategy,
                    result: Some("Changed by another writer.".to_string()),
                },
            )
            .await?;
        Ok(())
    }

    /// Reads every notification up to the `turn/completed` of `turn_id`, or of the next turn
    /// other than `turn_id` when `next` is set.
    async fn collect(&mut self, turn_id: &str, next: bool) -> Result<TurnNotifications> {
        let mut notifications = Vec::new();
        loop {
            let JSONRPCMessage::Notification(notification) = self.server.read_next_message().await?
            else {
                continue;
            };
            let completed = (notification.method == "turn/completed")
                .then(|| {
                    serde_json::from_value::<TurnCompletedNotification>(
                        notification.params.clone()?,
                    )
                    .ok()
                })
                .flatten()
                .filter(|completed| (completed.turn.id == turn_id) != next);
            notifications.push(notification);
            if let Some(completed) = completed {
                return Ok(TurnNotifications {
                    completed,
                    notifications,
                });
            }
        }
    }

    async fn collected_turn(&mut self) -> Result<TurnNotifications> {
        let turn_id = self.start_turn().await?;
        self.collect(&turn_id, /*next*/ false).await
    }
}

/// A status-only final reply after this turn's own commit warns once with the match variant
/// on every commit route: ordinary completion (R1), an explicit blocked update (R2), a
/// completion that declares open issues (R3), and repeated refused completions that the host
/// blocks (R4). Nothing retries and the committed run is unchanged by finalization.
#[tokio::test]
async fn status_only_after_commit_warns_per_route() -> Result<()> {
    enum Route {
        Completion,
        BlockedUpdate,
        OpenIssues,
        RefusedCompletions,
    }
    for route in [
        Route::Completion,
        Route::BlockedUpdate,
        Route::OpenIssues,
        Route::RefusedCompletions,
    ] {
        let mut harness = harness_in(CHECK_SANDBOX).await?;
        let run = harness.start_run(StatefulWorkflowMode::Collaborative).await?;
        let revision = run.revision;
        let (name, status, mut script) = match route {
            Route::Completion => {
                harness.seed_plan(&run.id).await?;
                (
                    "R1",
                    StatefulRunStatus::Completed,
                    vec![
                        super::stateful_acceptance_support::run_check(
                            "check",
                            harness.project_root.path(),
                        ),
                        complete("complete", revision, &[]),
                    ],
                )
            }
            Route::BlockedUpdate => (
                "R2",
                StatefulRunStatus::Blocked,
                vec![block("block", revision)],
            ),
            Route::OpenIssues => (
                "R3",
                StatefulRunStatus::Blocked,
                vec![complete(
                    "complete",
                    revision,
                    &["The parser source was not read."],
                )],
            ),
            Route::RefusedCompletions => (
                "R4",
                StatefulRunStatus::Blocked,
                (1..=3)
                    .map(|attempt| complete(&format!("complete-{attempt}"), revision, &[]))
                    .collect(),
            ),
        };
        script.push(message("final", STATUS_LINE));
        let scripted = script.len();
        let mock = responses::mount_sse_sequence(&harness.responses_server, script).await;

        let turn = harness.collected_turn().await?;

        let durable = harness.read(&run.id).await?.run.expect("run");
        assert_eq!(
            (
                turn.completed.turn.status.clone(),
                durable.status,
                turn.last_run_update_revision(),
                turn.warnings(),
                mock.requests().len(),
            ),
            (
                TurnStatus::Completed,
                status,
                Some(durable.revision),
                vec![matched_warning(STATUS_LINE, &durable)],
                scripted,
            ),
            "{name}"
        );
    }
    Ok(())
}

/// A running Autonomous turn that ends with a status line and no Stateful call warns with
/// the exact base sentence and quotes no result; the run stays running and the host's
/// ordinary continuation still follows.
#[tokio::test]
async fn running_continuation_status_only_warns_without_result() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
    let mock = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![message("r1", STATUS_LINE), message("r2", ANSWER)],
    )
    .await;

    let turn = harness.collected_turn().await?;
    let continuation = harness
        .collect(&turn.completed.turn.id, /*next*/ true)
        .await?;

    // The continuation proves the run was still running after the status-only turn; that
    // turn's finalization wrote nothing to the run.
    assert_eq!(
        (
            turn.completed.turn.status.clone(),
            turn.warnings(),
            // Only the continuation claim may report the run, at its unchanged revision.
            turn.last_run_update_revision()
                .filter(|revision| *revision != run.revision),
            continuation.warnings(),
            harness.read(&run.id).await?.host_answer,
            mock.requests().len(),
        ),
        (
            TurnStatus::Completed,
            vec![STATUS_ONLY.to_string()],
            None,
            Vec::<String>::new(),
            None,
            2,
        )
    );
    Ok(())
}

/// An accepted answer block whose body is only a status line still ends the run answered
/// (unchanged) and warns with the base sentence.
#[tokio::test]
async fn answered_status_only_body_warns_and_stays_answered() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
    let mock = responses::mount_sse_once(
        &harness.responses_server,
        message("r1", &format!("Done.\n\n{BLOCK}")),
    )
    .await;

    let turn = harness.collected_turn().await?;

    let read = harness.read(&run.id).await?;
    assert_eq!(
        (
            turn.completed.turn.status.clone(),
            read.run.expect("run").status,
            read.host_answer.map(|answer| answer.turn_id),
            turn.warnings(),
            mock.requests().len(),
        ),
        (
            TurnStatus::Completed,
            StatefulRunStatus::Answered,
            Some(turn.completed.turn.id.clone()),
            vec![STATUS_ONLY.to_string()],
            1,
        )
    );
    Ok(())
}

/// A run that was already blocked when the turn started, with no commit in the turn, gets
/// the base sentence: its earlier result is never attributed to this turn.
#[tokio::test]
async fn already_blocked_at_start_quotes_no_result() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Collaborative).await?;
    harness
        .write_run_elsewhere(&run.id, codex_stateful_runtime::StatefulRunStatus::Blocked)
        .await?;
    let before = harness.read(&run.id).await?.run.expect("run");
    let mock = responses::mount_sse_once(&harness.responses_server, message("r1", "Done.")).await;

    let turn = harness.collected_turn().await?;

    assert_eq!(
        (
            turn.completed.turn.status.clone(),
            harness.read(&run.id).await?.run.expect("run"),
            turn.warnings(),
            mock.requests().len(),
        ),
        (
            TurnStatus::Completed,
            before,
            vec![STATUS_ONLY.to_string()],
            1,
        )
    );
    Ok(())
}

/// When another writer changes the run after this turn's commit and before finalization,
/// the warning names the turn's own commit but quotes no result.
#[tokio::test]
async fn revision_change_after_commit_omits_result() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Collaborative).await?;
    let mock = responses::mount_response_sequence(
        &harness.responses_server,
        vec![
            responses::sse_response(block("block", run.revision)),
            held(message("final", STATUS_LINE)),
        ],
    )
    .await;
    let turn_id = harness.start_turn().await?;
    // The final reply is held while the turn's commit lands; then another writer moves on.
    let store = harness.store().await?;
    let run_id = codex_stateful_runtime::StatefulRunId::parse(run.id.as_str())?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while store.get_run(&run_id).await.ok().flatten().is_none_or(|current| {
            current.status != codex_stateful_runtime::StatefulRunStatus::Blocked
        }) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let committed = store.get_run(&run_id).await?.expect("run").revision;
    harness
        .write_run_elsewhere(&run.id, codex_stateful_runtime::StatefulRunStatus::Blocked)
        .await?;

    let turn = harness.collect(&turn_id, /*next*/ false).await?;

    let durable = harness.read(&run.id).await?.run.expect("run");
    assert_eq!(
        (
            turn.completed.turn.status.clone(),
            durable.revision,
            durable.result.as_deref(),
            turn.warnings(),
            mock.requests().len(),
        ),
        (
            TurnStatus::Completed,
            committed + 1,
            Some("Changed by another writer."),
            vec![format!(
                "This final reply may omit the deliverable: it contains only a status line. This turn recorded the run as blocked at revision {committed}; it has changed since, so its result is not quoted."
            )],
            2,
        )
    );
    Ok(())
}

/// A substantive final reply after this turn's completion never warns.
#[tokio::test]
async fn substantive_final_after_commit_does_not_warn() -> Result<()> {
    let mut harness = harness_in(CHECK_SANDBOX).await?;
    let run = harness.start_run(StatefulWorkflowMode::Collaborative).await?;
    harness.seed_plan(&run.id).await?;
    let mock = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            super::stateful_acceptance_support::run_check("check", harness.project_root.path()),
            complete("complete", run.revision, &[]),
            message("final", "42"),
        ],
    )
    .await;

    let turn = harness.collected_turn().await?;

    assert_eq!(
        (
            turn.completed.turn.status.clone(),
            harness.status(&run.id).await?,
            turn.warnings(),
            mock.requests().len(),
        ),
        (
            TurnStatus::Completed,
            StatefulRunStatus::Completed,
            Vec::<String>::new(),
            3,
        )
    );
    Ok(())
}

/// A running turn whose final reply is substantive, or whose last message is commentary,
/// never warns.
#[tokio::test]
async fn running_substantive_and_commentary_do_not_warn() -> Result<()> {
    for last in [
        message("r1", "The bug was an off-by-one; fixed."),
        commentary("r1", STATUS_LINE),
    ] {
        let mut harness = harness().await?;
        harness.start_run(StatefulWorkflowMode::Autonomous).await?;
        let mock = responses::mount_sse_sequence(
            &harness.responses_server,
            vec![last.clone(), message("r2", ANSWER)],
        )
        .await;

        let turn = harness.collected_turn().await?;
        let continuation = harness
            .collect(&turn.completed.turn.id, /*next*/ true)
            .await?;

        assert_eq!(
            (
                turn.completed.turn.status.clone(),
                turn.warnings(),
                continuation.warnings(),
                mock.requests().len(),
            ),
            (
                TurnStatus::Completed,
                Vec::<String>::new(),
                Vec::<String>::new(),
                2
            ),
            "{last}"
        );
    }
    Ok(())
}

/// A real answer with a valid outcome block ends the run answered and never warns.
#[tokio::test]
async fn answered_substantive_body_does_not_warn() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
    let mock = responses::mount_sse_once(&harness.responses_server, message("r1", &ready())).await;

    let turn = harness.collected_turn().await?;

    assert_eq!(
        (
            turn.completed.turn.status.clone(),
            harness.status(&run.id).await?,
            turn.warnings(),
            mock.requests().len(),
        ),
        (
            TurnStatus::Completed,
            StatefulRunStatus::Answered,
            Vec::<String>::new(),
            1,
        )
    );
    Ok(())
}
