//! Core lifecycle wiring of host-ended answer reservations: a test contributor grants a
//! reservation and finalizes like a run owner, so these tests observe exactly what Core
//! records and when it calls finalization, through the real task lifecycle.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_core::TurnInputRequest;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::HostAnswerDisqualifier;
use codex_extension_api::HostAnswerPhase;
use codex_extension_api::HostAnswerReservation;
use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::TurnStartInput;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::turn_input::NotSubmittedReason;
use codex_protocol::turn_input::SteerSubmission;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Notify;
use wiremock::MockServer;

/// Marks the user input of a turn that gets a reservation.
const CANDIDATE: &str = "[candidate]";
const ANSWER: &str = "parse_config returns Result<Config, Error>.";

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
}

impl Gate {
    async fn pass(&self) {
        self.entered.notify_one();
        self.release.notified().await;
    }
}

/// Grants a reservation to every turn whose input carries [`CANDIDATE`], and finalizes as a
/// run owner would: authorize, then resolve as committed.
#[derive(Default)]
struct Owner {
    reservations: Mutex<Vec<Arc<HostAnswerReservation>>>,
    finalized: Mutex<Vec<String>>,
    authorizations: Mutex<Vec<bool>>,
    before_authorize: Option<Gate>,
    after_authorize: Option<Gate>,
}

impl Owner {
    fn reservation(&self) -> Arc<HostAnswerReservation> {
        self.reservations
            .lock()
            .expect("reservations")
            .last()
            .cloned()
            .expect("a candidate turn started")
    }

    fn finalized(&self) -> Vec<String> {
        self.finalized.lock().expect("finalized").clone()
    }

    fn authorizations(&self) -> Vec<bool> {
        self.authorizations.lock().expect("authorizations").clone()
    }
}

impl TurnLifecycleContributor for Owner {
    fn on_turn_start<'a>(&'a self, input: TurnStartInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let candidate = input.user_input.iter().any(
                |item| matches!(item, UserInput::Text { text, .. } if text.contains(CANDIDATE)),
            );
            if !candidate {
                return;
            }
            input.turn_store.insert(HostAnswerReservation::new(
                "run".to_string(),
                "thread".to_string(),
                input.turn_id.to_string(),
            ));
            let reservation = input
                .turn_store
                .get::<HostAnswerReservation>()
                .expect("inserted reservation");
            self.reservations
                .lock()
                .expect("reservations")
                .push(reservation);
        })
    }

    fn on_turn_finalize<'a>(
        &'a self,
        input: TurnFinalizeInput<'a>,
    ) -> ExtensionFuture<'a, TurnFinalizeOutcome> {
        Box::pin(async move {
            self.finalized
                .lock()
                .expect("finalized")
                .push(input.last_agent_message.to_string());
            if let Some(gate) = &self.before_authorize {
                gate.pass().await;
            }
            let authorized = input.reservation.authorize_commit();
            self.authorizations
                .lock()
                .expect("authorizations")
                .push(authorized);
            if !authorized {
                return TurnFinalizeOutcome::Declined("aborted".to_string());
            }
            if let Some(gate) = &self.after_authorize {
                gate.pass().await;
            }
            input.reservation.resolve_commit(/*committed*/ true);
            TurnFinalizeOutcome::Committed
        })
    }
}

async fn build(server: &MockServer, owner: Arc<Owner>) -> anyhow::Result<TestCodex> {
    build_with(server, owner, |_| {}).await
}

async fn build_with(
    server: &MockServer,
    owner: Arc<Owner>,
    configure: impl FnOnce(&mut codex_core::config::Config) + Send + 'static,
) -> anyhow::Result<TestCodex> {
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.turn_lifecycle_contributor(owner);
    let mut builder = test_codex()
        .with_extensions(Arc::new(extensions.build()))
        .with_config(configure);
    builder.build_with_auto_env(server).await
}

async fn start_turn(test: &TestCodex, text: &str) -> anyhow::Result<()> {
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    Ok(())
}

async fn turn_complete(test: &TestCodex) -> Option<String> {
    let EventMsg::TurnComplete(complete) = wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await
    else {
        unreachable!();
    };
    complete.last_agent_message
}

fn answer(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

/// Waits until the mock received `count` model requests.
async fn wait_for_requests(mock: &responses::ResponseMock, count: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while mock.requests().len() < count {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model request received");
}

/// A held model response: the turn stays answering until the delay passes.
fn held(body: String) -> wiremock::ResponseTemplate {
    responses::sse_response(body).set_delay(Duration::from_secs(3))
}

async fn shell(test: &TestCodex, command: &str) -> anyhow::Result<Result<(), String>> {
    let (reply, admitted) = tokio::sync::oneshot::channel();
    test.codex
        .submit(Op::RunUserShellCommand {
            command: command.to_string(),
            timeout_ms: None,
            reply: Some(reply),
        })
        .await?;
    Ok(admitted.await?.map_err(|error| error.to_string()))
}

/// T01 (Core): a turn that only answers reaches finalization once, after its last response,
/// with its exact final message, and completes normally.
#[tokio::test]
async fn text_only_answer_is_finalized_with_its_exact_message() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
    let owner = Arc::new(Owner::default());
    let test = build(&server, Arc::clone(&owner)).await?;

    start_turn(
        &test,
        &format!("What does parse_config return? {CANDIDATE}"),
    )
    .await?;
    assert_eq!(turn_complete(&test).await.as_deref(), Some(ANSWER));

    assert_eq!(mock.requests().len(), 1);
    assert_eq!(owner.finalized(), vec![ANSWER.to_string()]);
    assert_eq!(owner.authorizations(), vec![true]);
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Committed);
    Ok(())
}

/// T03/T04/T06: every kind of call, including unknown and malformed ones that never run,
/// disqualifies at its first observation; the later answer is never finalized.
#[tokio::test]
async fn every_observed_call_kind_disqualifies_the_answer() -> anyhow::Result<()> {
    let call_responses: Vec<(&str, Vec<serde_json::Value>)> = vec![
        (
            "unknown function",
            vec![responses::ev_function_call("call-1", "no_such_tool", "{}")],
        ),
        (
            "malformed completion",
            vec![responses::ev_function_call(
                "call-1",
                "stateful_run_update",
                "{not json",
            )],
        ),
        (
            "custom tool",
            vec![responses::ev_custom_tool_call(
                "call-1",
                "no_such_custom",
                "input",
            )],
        ),
        (
            "tool search",
            vec![responses::ev_tool_search_call(
                "call-1",
                &json!({"query": "anything"}),
            )],
        ),
    ];
    for (case, events) in call_responses {
        let server = responses::start_mock_server().await;
        let mut first = vec![responses::ev_response_created("r1")];
        first.extend(events);
        first.push(responses::ev_completed("r1"));
        let mock = responses::mount_sse_sequence(
            &server,
            vec![responses::sse(first), answer("r2", ANSWER)],
        )
        .await;
        let owner = Arc::new(Owner::default());
        let test = build(&server, Arc::clone(&owner)).await?;
        start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
        assert_eq!(
            turn_complete(&test).await.as_deref(),
            Some(ANSWER),
            "{case}"
        );
        assert_eq!(mock.requests().len(), 2, "{case}");
        assert_eq!(owner.finalized(), Vec::<String>::new(), "{case}");
        assert_eq!(
            owner.reservation().phase(),
            HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ToolCall),
            "{case}"
        );
    }

    // Hosted calls the provider runs itself, before or after the answer in one response.
    let hosted: Vec<(&str, Vec<serde_json::Value>)> = vec![
        (
            "web search before",
            vec![
                responses::ev_web_search_call_done("ws-1", "completed", "weather"),
                responses::ev_assistant_message("m1", ANSWER),
            ],
        ),
        (
            "web search added only, after the answer",
            vec![
                responses::ev_assistant_message("m1", ANSWER),
                responses::ev_web_search_call_added_partial("ws-1", "in_progress"),
            ],
        ),
        (
            "image generation",
            vec![
                responses::ev_image_generation_call("ig-1", "completed", "a cat", "Zm9v"),
                responses::ev_assistant_message("m1", ANSWER),
            ],
        ),
    ];
    for (case, events) in hosted {
        let server = responses::start_mock_server().await;
        let mut body = vec![responses::ev_response_created("r1")];
        body.extend(events);
        body.push(responses::ev_completed("r1"));
        responses::mount_sse_once(&server, responses::sse(body)).await;
        let owner = Arc::new(Owner::default());
        let test = build(&server, Arc::clone(&owner)).await?;
        start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
        turn_complete(&test).await;
        assert_eq!(owner.finalized(), Vec::<String>::new(), "{case}");
        assert_eq!(
            owner.reservation().phase(),
            HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ToolCall),
            "{case}"
        );
    }
    Ok(())
}

/// T08: a call and an answer in one response disqualify in either order, without holding
/// the response.
#[tokio::test]
async fn call_and_answer_in_one_response_disqualify_in_either_order() -> anyhow::Result<()> {
    for answer_first in [true, false] {
        let server = responses::start_mock_server().await;
        let message = responses::ev_assistant_message("m1", ANSWER);
        let call = responses::ev_function_call("call-1", "no_such_tool", "{}");
        let (a, b) = if answer_first {
            (message, call)
        } else {
            (call, message)
        };
        responses::mount_sse_sequence(
            &server,
            vec![
                responses::sse(vec![
                    responses::ev_response_created("r1"),
                    a,
                    b,
                    responses::ev_completed("r1"),
                ]),
                answer("r2", ANSWER),
            ],
        )
        .await;
        let owner = Arc::new(Owner::default());
        let test = build(&server, Arc::clone(&owner)).await?;
        start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
        turn_complete(&test).await;
        assert_eq!(owner.finalized(), Vec::<String>::new());
        assert_eq!(
            owner.reservation().phase(),
            HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ToolCall)
        );
    }
    Ok(())
}

/// T05: a hosted call observed only as added, on a response that then fails, stays
/// recorded through the retry that answers; a failed response alone also disqualifies.
#[tokio::test]
async fn failed_responses_and_their_hosted_calls_stay_disqualifying() -> anyhow::Result<()> {
    let cases = [
        (
            vec![
                responses::ev_response_created("r1"),
                responses::ev_web_search_call_added_partial("ws-1", "in_progress"),
            ],
            HostAnswerDisqualifier::ToolCall,
        ),
        (
            vec![responses::ev_response_created("r1")],
            HostAnswerDisqualifier::ResponseFailed,
        ),
    ];
    for (failed, reason) in cases {
        let server = responses::start_mock_server().await;
        // The first stream ends before response.completed; the retry answers.
        responses::mount_sse_sequence(&server, vec![responses::sse(failed), answer("r2", ANSWER)])
            .await;
        let owner = Arc::new(Owner::default());
        let test = build_with(&server, Arc::clone(&owner), |config| {
            config.model_provider.stream_max_retries = Some(1);
        })
        .await?;
        start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
        assert_eq!(turn_complete(&test).await.as_deref(), Some(ANSWER));
        assert_eq!(owner.finalized(), Vec::<String>::new());
        assert_eq!(
            owner.reservation().phase(),
            HostAnswerPhase::Disqualified(reason)
        );
    }
    Ok(())
}

/// T14 (legacy notify): an executable hook set disqualifies at task start, before any hook
/// could run.
#[tokio::test]
async fn an_executable_hook_set_disqualifies_at_task_start() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
    let owner = Arc::new(Owner::default());
    let notify = if cfg!(windows) {
        vec!["cmd".to_string(), "/c".to_string(), "exit 0".to_string()]
    } else {
        vec!["true".to_string()]
    };
    let test = build_with(&server, Arc::clone(&owner), move |config| {
        config.notify = Some(notify);
    })
    .await?;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    turn_complete(&test).await;
    assert_eq!(owner.finalized(), Vec::<String>::new());
    assert_eq!(
        owner.reservation().phase(),
        HostAnswerPhase::Disqualified(HostAnswerDisqualifier::ExecutableHooks)
    );
    Ok(())
}

/// T19: an interrupt during inference aborts the reservation; nothing is finalized.
#[tokio::test]
async fn interrupt_during_inference_aborts_without_finalizing() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_once(&server, held(answer("r1", ANSWER))).await;
    let owner = Arc::new(Owner::default());
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    wait_for_requests(&mock, 1).await;
    test.codex.submit(Op::Interrupt).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    assert_eq!(owner.finalized(), Vec::<String>::new());
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Aborted);
    Ok(())
}

/// T20: an interrupt while the finalizer waits (before commit authorization) wins: the
/// task is cancelled and the commit is never authorized.
#[tokio::test]
async fn interrupt_while_finalizer_waits_wins_before_authorization() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
    let owner = Arc::new(Owner {
        before_authorize: Some(Gate::default()),
        ..Owner::default()
    });
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    let gate = owner.before_authorize.as_ref().expect("gate");
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Finalizing);
    test.codex.submit(Op::Interrupt).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    gate.release.notify_one();
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Aborted);
    assert!(
        !owner.reservation().authorize_commit(),
        "an aborted reservation can never be authorized"
    );
    assert_eq!(owner.authorizations(), Vec::<bool>::new());
    Ok(())
}

/// T21: an interrupt that arrives after commit authorization does not cancel the task: it
/// waits, the commit resolves, and the turn completes; no abort is reported.
#[tokio::test]
async fn interrupt_after_authorization_waits_for_the_completed_turn() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
    let owner = Arc::new(Owner {
        after_authorize: Some(Gate::default()),
        ..Owner::default()
    });
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    let gate = owner.after_authorize.as_ref().expect("gate");
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Committing);
    test.codex.submit(Op::Interrupt).await?;
    // The interrupt must not cancel a task whose commit was authorized.
    let early = tokio::time::timeout(
        Duration::from_millis(300),
        wait_for_event(&test.codex, |event| {
            matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
        }),
    )
    .await;
    assert!(early.is_err(), "the turn ended before its commit resolved");
    gate.release.notify_one();
    let ended = wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
    })
    .await;
    assert!(matches!(ended, EventMsg::TurnComplete(_)), "{ended:?}");
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Committed);
    assert_eq!(owner.authorizations(), vec![true]);
    Ok(())
}

/// T09/T10/T12: a user shell command is refused before spawn while the candidate answers
/// and while it finalizes; once the turn has ended, a command runs as a new turn.
#[tokio::test]
async fn user_shell_is_refused_while_answering_and_finalizing() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_once(&server, held(answer("r1", ANSWER))).await;
    let owner = Arc::new(Owner {
        before_authorize: Some(Gate::default()),
        ..Owner::default()
    });
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    wait_for_requests(&mock, 1).await;

    let refused = shell(&test, "echo answering").await?;
    let message = refused.expect_err("refused while answering");
    assert!(
        message.contains("no command can start beside it"),
        "{message}"
    );

    let gate = owner.before_authorize.as_ref().expect("gate");
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    let refused = shell(&test, "echo finalizing").await?;
    assert!(refused.is_err(), "refused while finalizing");
    gate.release.notify_one();
    assert_eq!(turn_complete(&test).await.as_deref(), Some(ANSWER));
    assert_eq!(owner.reservation().phase(), HostAnswerPhase::Committed);

    // After the handoff a command runs as its own turn.
    shell(&test, "echo after")
        .await?
        .expect("admitted after the turn");
    let EventMsg::ExecCommandEnd(end) = wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ExecCommandEnd(_))
    })
    .await
    else {
        unreachable!();
    };
    assert_ne!(end.turn_id, owner.reservation().turn_id());
    Ok(())
}

/// T28: steering before the input closes is answered by the same task; after closure it is
/// refused as steering, so it belongs to a later turn.
#[tokio::test]
async fn steering_is_owned_by_the_task_only_until_its_input_closes() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(
        &server,
        vec![
            held(answer("r1", "First answer.")),
            responses::sse_response(answer("r2", ANSWER)),
        ],
    )
    .await;
    let owner = Arc::new(Owner {
        before_authorize: Some(Gate::default()),
        ..Owner::default()
    });
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    wait_for_requests(&mock, 1).await;
    let turn_id = owner.reservation().turn_id().to_string();
    let steered = test
        .codex
        .steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "Also mention the error type.".to_string(),
                text_elements: Vec::new(),
            }]),
            turn_id.clone(),
        )
        .await?;
    assert_eq!(
        steered,
        SteerSubmission::Steered {
            turn_id: turn_id.clone()
        }
    );

    let gate = owner.before_authorize.as_ref().expect("gate");
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    assert_eq!(
        mock.requests().len(),
        2,
        "the steer was answered by the same task"
    );
    assert_eq!(owner.finalized(), vec![ANSWER.to_string()]);
    let late = test
        .codex
        .steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "One more thing.".to_string(),
                text_elements: Vec::new(),
            }]),
            turn_id,
        )
        .await?;
    assert_eq!(
        late,
        SteerSubmission::NotSubmitted {
            reason: NotSubmittedReason::NoActiveTurn
        }
    );
    gate.release.notify_one();
    turn_complete(&test).await;
    Ok(())
}

/// T11/T17: a user shell command admitted beside an earlier turn and still running when
/// the candidate task starts disqualifies it.
#[tokio::test]
async fn outstanding_earlier_shell_work_disqualifies_the_next_candidate() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_response_sequence(
        &server,
        vec![
            held(answer("r1", "Earlier answer.")),
            responses::sse_response(answer("r2", ANSWER)),
        ],
    )
    .await;
    let owner = Arc::new(Owner::default());
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, "An earlier, ordinary turn.").await?;
    wait_for_requests(&mock, 1).await;
    let long_command = if cfg!(windows) {
        "ping -n 8 127.0.0.1"
    } else {
        "sleep 6"
    };
    shell(&test, long_command)
        .await?
        .expect("admitted beside an ordinary turn");
    turn_complete(&test).await;

    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    turn_complete(&test).await;
    assert_eq!(owner.finalized(), Vec::<String>::new());
    assert_eq!(
        owner.reservation().phase(),
        HostAnswerPhase::Disqualified(HostAnswerDisqualifier::OutstandingWork)
    );
    Ok(())
}

/// T07: compaction during the candidate task (here the pre-turn compaction an earlier turn's
/// token usage triggers) disqualifies it, whatever history it leaves behind.
#[tokio::test]
async fn compaction_disqualifies_the_answer() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_assistant_message("m1", "Earlier answer."),
                responses::ev_completed_with_tokens("r1", /*total_tokens*/ 330_000),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("m2", "Summary of the conversation."),
                responses::ev_completed_with_tokens("r2", /*total_tokens*/ 200),
            ]),
            responses::sse(vec![
                responses::ev_assistant_message("m3", ANSWER),
                responses::ev_completed_with_tokens("r3", /*total_tokens*/ 120),
            ]),
        ],
    )
    .await;
    let owner = Arc::new(Owner::default());
    let mut provider =
        codex_model_provider_info::built_in_model_providers(/*openai_base_url*/ None)["openai"]
            .clone();
    provider.name = "OpenAI (test)".into();
    provider.base_url = Some(format!("{}/v1", server.uri()));
    provider.supports_websockets = false;
    let test = build_with(&server, Arc::clone(&owner), move |config| {
        config.model_provider = provider;
        config.model_auto_compact_token_limit = Some(200_000);
    })
    .await?;
    start_turn(&test, "An earlier, ordinary turn.").await?;
    turn_complete(&test).await;
    start_turn(&test, &format!("Answer. {CANDIDATE}")).await?;
    assert_eq!(turn_complete(&test).await.as_deref(), Some(ANSWER));
    assert_eq!(mock.requests().len(), 3);
    assert_eq!(owner.finalized(), Vec::<String>::new());
    assert_eq!(
        owner.reservation().phase(),
        HostAnswerPhase::Disqualified(HostAnswerDisqualifier::Compaction)
    );
    Ok(())
}
