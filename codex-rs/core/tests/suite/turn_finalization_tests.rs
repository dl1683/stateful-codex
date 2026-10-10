//! Task-owned turn finalization through the real task lifecycle: a test contributor records a
//! terminal outcome the way a Stateful run owner does (authorize immediately before its
//! commit), so these tests observe how Core orders that commit against interrupts.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use codex_core::TurnInputRequest;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::TurnFinalizeInput;
use codex_extension_api::TurnFinalizeOutcome;
use codex_extension_api::TurnLifecycleContributor;
use codex_protocol::AgentPath;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use tokio::sync::Notify;
use wiremock::MockServer;

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

/// Finalizes like a run owner: optionally waits, authorizes, optionally waits, then reports
/// `outcome` when authorized.
struct Owner {
    finalized: Mutex<Vec<String>>,
    authorizations: Mutex<Vec<bool>>,
    before_authorize: Option<Gate>,
    after_authorize: Option<Gate>,
    outcome: TurnFinalizeOutcome,
}

impl Default for Owner {
    fn default() -> Self {
        Self {
            finalized: Mutex::default(),
            authorizations: Mutex::default(),
            before_authorize: None,
            after_authorize: None,
            outcome: TurnFinalizeOutcome::Recorded,
        }
    }
}

impl Owner {
    fn finalized(&self) -> Vec<String> {
        self.finalized.lock().expect("finalized").clone()
    }

    fn authorizations(&self) -> Vec<bool> {
        self.authorizations.lock().expect("authorizations").clone()
    }
}

impl TurnLifecycleContributor for Owner {
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
            let authorized = (input.authorize_commit)();
            self.authorizations
                .lock()
                .expect("authorizations")
                .push(authorized);
            if !authorized {
                return TurnFinalizeOutcome::NotHandled;
            }
            if let Some(gate) = &self.after_authorize {
                gate.pass().await;
            }
            self.outcome.clone()
        })
    }
}

async fn build(server: &MockServer, owner: Arc<Owner>) -> anyhow::Result<TestCodex> {
    let mut extensions = ExtensionRegistryBuilder::new();
    extensions.turn_lifecycle_contributor(owner);
    let mut builder = test_codex().with_extensions(Arc::new(extensions.build()));
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

fn answer(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_response_created(id),
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

async fn turn_end(test: &TestCodex) -> EventMsg {
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnAborted(_) | EventMsg::TurnComplete(_))
    })
    .await
}

/// Asserts that the turn does not end while its commit is held.
async fn assert_no_turn_end(test: &TestCodex) {
    let early = tokio::time::timeout(Duration::from_millis(300), turn_end(test)).await;
    assert!(
        early.is_err(),
        "the turn ended while its commit was held: {early:?}"
    );
}

/// A turn that ends normally is finalized with its exact final message, inside the task.
#[tokio::test]
async fn a_finished_turn_is_finalized_with_its_final_message() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
    let owner = Arc::new(Owner::default());
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, "What does parse_config return?").await?;
    assert!(matches!(turn_end(&test).await, EventMsg::TurnComplete(_)));
    assert_eq!(owner.finalized(), vec![ANSWER.to_string()]);
    assert_eq!(owner.authorizations(), vec![true]);
    Ok(())
}

/// An interrupt that arrives before the commit is authorized wins: the turn is aborted and
/// no commit is ever authorized.
#[tokio::test]
async fn an_interrupt_before_authorization_wins() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
    let owner = Arc::new(Owner {
        before_authorize: Some(Gate::default()),
        ..Owner::default()
    });
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, "What does parse_config return?").await?;
    let gate = owner.before_authorize.as_ref().expect("gate");
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    test.codex.submit(Op::Interrupt).await?;
    assert!(matches!(turn_end(&test).await, EventMsg::TurnAborted(_)));
    gate.release.notify_one();
    assert!(
        !owner.authorizations().contains(&true),
        "{:?}",
        owner.authorizations()
    );
    Ok(())
}

/// An interrupt that arrives after authorization does not cancel the task: it waits, and the
/// turn completes. An unknown commit outcome is reported as a warning, never as a cancellation.
#[tokio::test]
async fn an_interrupt_after_authorization_waits_for_the_completed_turn() -> anyhow::Result<()> {
    for outcome in [
        TurnFinalizeOutcome::Recorded,
        TurnFinalizeOutcome::Warning("the outcome is unknown".to_string()),
    ] {
        let server = responses::start_mock_server().await;
        responses::mount_sse_once(&server, answer("r1", ANSWER)).await;
        let owner = Arc::new(Owner {
            after_authorize: Some(Gate::default()),
            outcome: outcome.clone(),
            ..Owner::default()
        });
        let test = build(&server, Arc::clone(&owner)).await?;
        start_turn(&test, "What does parse_config return?").await?;
        let gate = owner.after_authorize.as_ref().expect("gate");
        tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
        test.codex.submit(Op::Interrupt).await?;
        assert_no_turn_end(&test).await;
        gate.release.notify_one();
        if let TurnFinalizeOutcome::Warning(message) = &outcome {
            wait_for_event(
                &test.codex,
                |event| matches!(event, EventMsg::Warning(warning) if &warning.message == message),
            )
            .await;
        }
        let ended = turn_end(&test).await;
        assert!(matches!(ended, EventMsg::TurnComplete(_)), "{ended:?}");
        assert_eq!(owner.authorizations(), vec![true]);
    }
    Ok(())
}

/// An interrupt that lost the race to an authorized commit never cancels the later turn that
/// the finishing task starts for queued mail.
#[tokio::test]
async fn a_losing_interrupt_never_cancels_the_next_turn() -> anyhow::Result<()> {
    let server = responses::start_mock_server().await;
    let mock = responses::mount_sse_sequence(
        &server,
        vec![answer("r1", ANSWER), answer("r2", "Mail read.")],
    )
    .await;
    let owner = Arc::new(Owner {
        after_authorize: Some(Gate::default()),
        ..Owner::default()
    });
    let test = build(&server, Arc::clone(&owner)).await?;
    start_turn(&test, "What does parse_config return?").await?;
    let gate = owner.after_authorize.as_ref().expect("gate");
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    test.codex
        .submit(Op::InterAgentCommunication {
            communication: InterAgentCommunication::new(
                AgentPath::root().join("child").expect("valid child path"),
                AgentPath::root(),
                Vec::new(),
                "Message Type: MESSAGE\nTask name: /root\nSender: /root/child\nPayload:\nchild done"
                    .to_string(),
                /*trigger_turn*/ true,
            ),
            start_options: Default::default(),
        })
        .await?;
    test.codex.submit(Op::Interrupt).await?;
    assert_no_turn_end(&test).await;
    gate.release.notify_one();

    let first = turn_end(&test).await;
    assert!(matches!(first, EventMsg::TurnComplete(_)), "{first:?}");
    // The mail turn finalizes too; let it through.
    tokio::time::timeout(Duration::from_secs(10), gate.entered.notified()).await?;
    gate.release.notify_one();
    let second = turn_end(&test).await;
    assert!(matches!(second, EventMsg::TurnComplete(_)), "{second:?}");
    assert_eq!(mock.requests().len(), 2);
    Ok(())
}
