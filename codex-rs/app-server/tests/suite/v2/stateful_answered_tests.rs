//! "Answered, unverified" through the public API: an Autonomous turn whose final answer ends
//! with the ready outcome block ends its run as Answered (never Completed), with the exact
//! answer and an explicit unverified basis; anything that is not a genuine, ready final
//! answer keeps the ordinary route.

use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCMessage;
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
use codex_stateful_runtime as runtime;
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

#[path = "stateful_answered_activation_tests.rs"]
mod activation;

#[path = "stateful_answered_persistence_tests.rs"]
mod persistence;

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

/// The admitted acceptance check runs outside the OS sandbox, which is not the subject here.
async fn harness() -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    super::stateful_acceptance_support::write_acceptance_files(project_root.path())?;
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode("danger-full-access")
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
        self.start_keyed_run(mode, "answered-run").await
    }

    async fn start_keyed_run(
        &mut self,
        mode: StatefulWorkflowMode,
        key: &str,
    ) -> Result<StatefulRun> {
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
        Ok(self.run(run_id).await?.status)
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

/// Explicit continuation, a declaration without an answer, commentary, and Collaborative
/// finals keep their ordinary route.
#[tokio::test]
async fn anything_but_a_genuine_ready_answer_keeps_the_ordinary_route() -> Result<()> {
    let continuing = format!(
        "I will read the parser next.\n\n{}",
        BLOCK.replace("disposition: answer", "disposition: continue")
    );
    let cases = [
        (StatefulWorkflowMode::Autonomous, message("r1", &continuing)),
        (StatefulWorkflowMode::Autonomous, message("r1", BLOCK)),
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
const COLLABORATIVE: StatefulWorkflowMode = StatefulWorkflowMode::Collaborative;
const UNREAD: &[&str] = &["The source was unread."];

/// A model call updating a new run (revision 1): completed with `open_issues`, or blocked.
fn run_update(call_id: &str, status: &str, open_issues: &[&str]) -> String {
    let mut arguments =
        serde_json::json!({"expectedRevision": 1, "status": status, "result": ANSWER});
    if status == "completed" {
        arguments["openIssues"] = serde_json::json!(open_issues);
        arguments["completionDisposition"] = serde_json::json!("noReusableLearning");
    }
    let arguments = arguments.to_string();
    responses::sse(vec![
        responses::ev_response_created(call_id),
        responses::ev_function_call(call_id, "stateful_run_update", &arguments),
        responses::ev_completed(call_id),
    ])
}

/// Serves `bodies` in order; the response at `gate` waits until the returned sender fires.
async fn mount_gated(
    server: &wiremock::MockServer,
    bodies: Vec<String>,
    gate: usize,
) -> std::sync::mpsc::Sender<()> {
    struct Gated(Vec<String>, usize, AtomicUsize, Mutex<mpsc::Receiver<()>>);
    impl wiremock::Respond for Gated {
        fn respond(&self, _: &wiremock::Request) -> wiremock::ResponseTemplate {
            let call = self.2.fetch_add(1, Ordering::SeqCst);
            if call == self.1 {
                let _ = self.3.lock().unwrap_or_else(PoisonError::into_inner).recv();
            }
            responses::sse_response(self.0[call].clone())
        }
    }
    let (release, open) = mpsc::channel();
    let gated = Gated(bodies, gate, AtomicUsize::new(0), Mutex::new(open));
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path_regex(".*/responses$"))
        .respond_with(gated)
        .mount(server)
        .await;
    release
}

type Outcome = (TurnStatus, Vec<String>, usize);

impl Harness {
    async fn run(&mut self, run_id: &str) -> Result<StatefulRun> {
        Ok(self.read(run_id).await?.run.expect("run"))
    }

    /// Another writer blocks the run with a new result, as a second client would.
    async fn block_elsewhere(&self, run_id: &str) -> Result<()> {
        let sqlite = SqliteConfig::new_for_testing(self.codex_home.path().abs());
        let store = runtime::StatefulRunStore::open(&sqlite).await?;
        let id = runtime::StatefulRunId::parse(run_id)?;
        let current = store.get_run(&id).await?.expect("run");
        let update = runtime::StatefulRunUpdate {
            expected_revision: current.revision,
            status: runtime::StatefulRunStatus::Blocked,
            strategy: current.strategy,
            result: Some("Changed by another writer.".to_string()),
        };
        store.update_run(&id, update).await?;
        Ok(())
    }

    /// Polls until the run reaches `status`, returning that durable snapshot.
    async fn committed(&mut self, run_id: &str, status: StatefulRunStatus) -> Result<StatefulRun> {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let run = self.run(run_id).await?;
                if run.status == status {
                    return Ok(run);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?
    }

    /// Collects turn `turn_id` (or, when `next`, the next other turn) up to its completion.
    async fn outcome(&mut self, turn_id: &str, next: bool) -> Result<Outcome> {
        let mut warnings = Vec::new();
        loop {
            let JSONRPCMessage::Notification(notification) =
                self.server.read_next_message().await?
            else {
                continue;
            };
            let params = notification.params.unwrap_or_default();
            if notification.method == "warning" {
                let message = serde_json::from_value::<WarningNotification>(params)?.message;
                if !message.starts_with(FALLBACK_METADATA_WARNING) {
                    warnings.push(message);
                }
            } else if notification.method == "turn/completed" {
                let completed = serde_json::from_value::<TurnCompletedNotification>(params)?;
                if (completed.turn.id == turn_id) != next {
                    let requests = self.responses_server.received_requests().await;
                    let requests = requests.unwrap_or_default().into_iter();
                    let requests = requests.filter(|call| call.url.path().ends_with("/responses"));
                    return Ok((completed.turn.status, warnings, requests.count()));
                }
            }
        }
    }
}

/// W1/C1: after this turn's own commit (R1-R4), a status-only final warns once with the match
/// variant and a real answer does not; the run read at its commit is unchanged afterwards.
#[tokio::test]
async fn status_only_after_commit_warns_per_route() -> Result<()> {
    for (name, status, last) in [
        ("R1", StatefulRunStatus::Completed, STATUS_LINE),
        ("C1", StatefulRunStatus::Completed, "42"),
        ("R2", StatefulRunStatus::Blocked, STATUS_LINE),
        ("R3", StatefulRunStatus::Blocked, STATUS_LINE),
        ("R4", StatefulRunStatus::Blocked, STATUS_LINE),
    ] {
        let mut harness = harness().await?;
        let run = harness.start_run(COLLABORATIVE).await?;
        let home = harness.codex_home.path();
        super::stateful_acceptance_support::seed_admitted_plan(
            home,
            &run.id,
            &[],
            harness.project_root.path(),
        )
        .await?;
        let root = harness.project_root.path();
        let mut script = match name {
            "R1" | "C1" => vec![
                super::stateful_acceptance_support::run_check("check", root),
                run_update("complete", "completed", &[]),
            ],
            "R2" => vec![run_update("block", "blocked", &[])],
            "R3" => vec![run_update("complete", "completed", UNREAD)],
            _ => (1..=3)
                .map(|attempt| run_update(&format!("complete-{attempt}"), "completed", &[]))
                .collect(),
        };
        script.push(message("final", last));
        let scripted = script.len();
        let release = mount_gated(&harness.responses_server, script, scripted - 1).await;
        let turn_id = harness.start_turn().await?;
        let snapshot = harness.committed(&run.id, status).await?;
        release.send(())?;

        let turn = harness.outcome(&turn_id, /*next*/ false).await?;

        let status = serde_json::to_value(snapshot.status)?;
        let warnings: Vec<_> = (last == STATUS_LINE).then(|| format!(
            "This final reply may omit the deliverable: it contains only a status line (\"{STATUS_LINE}\"). This turn's recorded result ({}, revision {}): {}",
            status.as_str().expect("status name"),
            snapshot.revision,
            snapshot.result.as_deref().unwrap_or_default()
        )).into_iter().collect();
        assert_eq!(
            (turn, harness.run(&run.id).await?),
            ((TurnStatus::Completed, warnings, scripted), snapshot),
            "{name}"
        );
    }
    Ok(())
}

/// W2/C2: a running turn with no commit warns with the base sentence only for a status line;
/// finalization writes nothing (the held continuation's claim is the only change).
#[tokio::test]
async fn running_turns_warn_only_for_status_lines() -> Result<()> {
    for (last, warnings) in [
        (message("r1", STATUS_LINE), vec![STATUS_ONLY.to_string()]),
        (
            message(
                "r1",
                &format!(
                    "Fixed an off-by-one.\n{}",
                    BLOCK.replace("disposition: answer", "disposition: continue")
                ),
            ),
            Vec::new(),
        ),
        (commentary("r1", STATUS_LINE), Vec::new()),
    ] {
        let mut harness = harness().await?;
        let started = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
        let script = vec![last.clone(), message("r2", ANSWER)];
        let release = mount_gated(&harness.responses_server, script, /*gate*/ 1).await;
        let turn_id = harness.start_turn().await?;
        let turn = harness.outcome(&turn_id, /*next*/ false).await?;
        // The continuation is claimed after the turn completes; wait for that claim (its model
        // response is still held), then read the run.
        let read = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let read = harness.read(&started.id).await?;
                if read
                    .run
                    .as_ref()
                    .is_some_and(|run| run.continuations_used == 1)
                {
                    return anyhow::Ok(read);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        let run = read.run.expect("run");
        release.send(())?;
        let continuation = harness.outcome(&turn_id, /*next*/ true).await?;

        assert_eq!(
            (
                turn,
                (run.status, run.revision, run.continuations_used, run.result),
                read.host_answer,
                continuation,
            ),
            (
                (TurnStatus::Completed, warnings, 1),
                (StatefulRunStatus::Running, started.revision + 1, 1, None),
                None,
                (TurnStatus::Completed, Vec::new(), 2),
            ),
            "{last}"
        );
    }
    Ok(())
}

/// W3/C3: an accepted block ends the run answered with its exact answer; only "Done." warns.
#[tokio::test]
async fn answered_bodies_warn_only_when_status_only() -> Result<()> {
    for (body, warnings) in [(ANSWER, Vec::new())] {
        let mut harness = harness().await?;
        let run = harness.start_run(StatefulWorkflowMode::Autonomous).await?;
        let answer = format!("{body}\n\n{BLOCK}");
        responses::mount_sse_once(&harness.responses_server, message("r1", &answer)).await;
        let turn_id = harness.start_turn().await?;

        let turn = harness.outcome(&turn_id, /*next*/ false).await?;

        let read = harness.read(&run.id).await?;
        let durable = read.run.expect("run");
        let host_answer = read.host_answer.expect("answer recorded");
        let expected_answer = codex_app_server_protocol::StatefulHostAnswer {
            outcome: codex_app_server_protocol::StatefulHostAnswerOutcome::Answered,
            turn_id,
            answer,
            basis: ANSWERED_UNVERIFIED_BASIS.to_string(),
            committed_at: host_answer.committed_at,
        };
        assert_eq!(
            (turn, durable.status, durable.result, host_answer),
            (
                (TurnStatus::Completed, warnings, 1),
                StatefulRunStatus::Answered,
                Some(body.to_string()),
                expected_answer,
            ),
            "{body}"
        );
    }
    Ok(())
}

/// A11(iv), W1: the public encoding matches admission, and legacy JSON defaults to Answered.
#[test]
fn published_answer_size_and_legacy_outcome() -> Result<()> {
    use codex_app_server_protocol::StatefulHostAnswer;
    use codex_app_server_protocol::StatefulHostAnswerOutcome;
    for (outcome, runtime_outcome) in [
        (
            StatefulHostAnswerOutcome::Answered,
            runtime::HostAnswerOutcome::Answered,
        ),
        (
            StatefulHostAnswerOutcome::Partial,
            runtime::HostAnswerOutcome::Partial,
        ),
    ] {
        let answer = StatefulHostAnswer {
            turn_id: "turn-\"1".to_string(),
            answer: "Quoted \"answer\"\nnext line".to_string(),
            basis: runtime_outcome.basis().to_string(),
            committed_at: i64::MIN,
            outcome,
        };
        assert_eq!(
            serde_json::to_vec(&answer)?.len(),
            runtime::serialized_answer_bytes(&answer.turn_id, &answer.answer, runtime_outcome)
        );
        let mut legacy = serde_json::to_value(&answer)?;
        legacy.as_object_mut().expect("object").remove("outcome");
        assert_eq!(
            serde_json::from_value::<StatefulHostAnswer>(legacy)?,
            StatefulHostAnswer {
                outcome: StatefulHostAnswerOutcome::Answered,
                ..answer
            }
        );
    }
    Ok(())
}

/// W5: a write after this turn's commit hides its result. The run's next status-only final
/// gets no second diagnostic: each run warns at most once.
#[tokio::test]
async fn changed_commit_quotes_no_result_and_the_run_warns_once() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(COLLABORATIVE).await?;
    let script = vec![
        run_update("block", "blocked", &[]),
        message("final", STATUS_LINE),
        message("next", "Done."),
    ];
    let release = mount_gated(&harness.responses_server, script, /*gate*/ 1).await;
    let turn_id = harness.start_turn().await?;
    let committed = harness
        .committed(&run.id, StatefulRunStatus::Blocked)
        .await?;
    harness.block_elsewhere(&run.id).await?;
    let changed = harness.run(&run.id).await?;
    release.send(())?;
    let changed_turn = harness.outcome(&turn_id, /*next*/ false).await?;
    let next_id = harness.start_turn().await?;
    let next_turn = harness.outcome(&next_id, /*next*/ false).await?;

    let not_quoted = format!(
        "This final reply may omit the deliverable: it contains only a status line. This turn recorded the run as blocked at revision {}; it has changed since, so its result is not quoted.",
        committed.revision
    );
    assert_eq!(
        (changed_turn, next_turn, harness.run(&run.id).await?),
        (
            (TurnStatus::Completed, vec![not_quoted], 2),
            (TurnStatus::Completed, Vec::new(), 3),
            changed,
        )
    );
    Ok(())
}

/// W4: a run already blocked when the turn starts, with no commit in the turn, gets the base
/// sentence and never its earlier result.
#[tokio::test]
async fn inherited_block_quotes_no_result() -> Result<()> {
    let mut harness = harness().await?;
    let run = harness.start_run(COLLABORATIVE).await?;
    harness.block_elsewhere(&run.id).await?;
    let blocked = harness.run(&run.id).await?;
    responses::mount_sse_once(&harness.responses_server, message("r1", "Done.")).await;
    let turn_id = harness.start_turn().await?;

    let turn = harness.outcome(&turn_id, /*next*/ false).await?;

    assert_eq!(
        (turn, harness.run(&run.id).await?),
        (
            (TurnStatus::Completed, vec![STATUS_ONLY.to_string()], 1),
            blocked
        )
    );
    Ok(())
}

/// After a run is cancelled, a genuinely new run on the same thread warns again.
#[tokio::test]
async fn a_new_run_on_the_same_thread_warns_again() -> Result<()> {
    let mut harness = harness().await?;
    let first = harness.start_run(COLLABORATIVE).await?;
    let script = vec![message("r1", "Done."), message("r2", "Done.")];
    responses::mount_sse_sequence(&harness.responses_server, script).await;
    let turn_id = harness.start_turn().await?;
    let first_turn = harness.outcome(&turn_id, /*next*/ false).await?;
    let params = codex_app_server_protocol::StatefulRunCancelParams {
        run_id: first.id.clone(),
        expected_revision: first.revision,
    };
    let _: codex_app_server_protocol::StatefulRunCancelResponse = harness
        .server
        .request(|request_id| ClientRequest::StatefulRunCancel { request_id, params })
        .await?;
    let second = harness.start_keyed_run(COLLABORATIVE, "second-run").await?;
    let turn_id = harness.start_turn().await?;
    let second_turn = harness.outcome(&turn_id, /*next*/ false).await?;

    let warned = |requests| {
        (
            TurnStatus::Completed,
            vec![STATUS_ONLY.to_string()],
            requests,
        )
    };
    assert_eq!(
        (first_turn, second_turn, second.id != first.id),
        (warned(1), warned(2), true)
    );
    Ok(())
}
