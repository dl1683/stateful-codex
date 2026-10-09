use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use codex_extension_api::ModelRequestContributor;
use codex_extension_api::ModelRequestInput;
use codex_extension_api::ModelRequestKind;
use codex_extension_api::ModelResponseError;
use codex_extension_api::ModelResponseStream;
use codex_extension_api::ResponseEvent;
use codex_protocol::models::ResponseItem;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::mpsc;

use super::HostActionObserver;
use super::MAX_HELD_BYTES;
use super::MAX_HELD_EVENTS;
use crate::services::ProjectIntelligenceServices;

const THREAD_ID: &str = "thread-1";

type Event = Result<ResponseEvent, ModelResponseError>;

struct Fixture {
    home: TempDir,
    services: ProjectIntelligenceServices,
    run_id: StatefulRunId,
}

fn services_at(home: &Path) -> ProjectIntelligenceServices {
    ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.abs()))
}

async fn create_run(services: &ProjectIntelligenceServices, id: &str) -> StatefulRunId {
    let run_id = StatefulRunId::parse(id).expect("run id");
    services
        .runtime()
        .await
        .expect("runtime opens")
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec![THREAD_ID.to_string()],
                goal: "What does the parser do?".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run created");
    run_id
}

async fn fixture() -> Fixture {
    let home = TempDir::new().expect("temporary state home");
    let services = services_at(home.path());
    let run_id = create_run(&services, "run-1").await;
    Fixture {
        home,
        services,
        run_id,
    }
}

impl Fixture {
    /// The run's (actions, completion attempts), read through a freshly opened store.
    async fn counts(&self) -> (u64, u64) {
        let ledger = services_at(self.home.path())
            .runtime()
            .await
            .expect("runtime reopens")
            .acceptance_ledger(&self.run_id)
            .await
            .expect("ledger");
        (ledger.host_actions, ledger.completion_attempts)
    }
}

fn fence(
    services: &ProjectIntelligenceServices,
    upstream: ModelResponseStream,
) -> ModelResponseStream {
    let mut metadata = None;
    HostActionObserver {
        services: services.clone(),
    }
    .request(ModelRequestInput {
        kind: ModelRequestKind::Generation,
        thread_id: THREAD_ID,
        client_metadata: &mut metadata,
        model: "test-model",
    })
    .expect("generation requests are fenced")
    .intercept(upstream)
}

/// An upstream response the test feeds event by event.
fn channel() -> (mpsc::UnboundedSender<Event>, ModelResponseStream) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let upstream = futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    (sender, Box::pin(upstream))
}

fn function_call(call_id: &str, name: &str, arguments: serde_json::Value) -> ResponseEvent {
    ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
        id: None,
        name: name.to_string(),
        namespace: None,
        arguments: arguments.to_string(),
        encrypted_function_args: None,
        call_id: call_id.to_string(),
        internal_chat_message_metadata_passthrough: None,
    })
}

fn completion() -> ResponseEvent {
    function_call(
        "complete",
        "stateful_run_update",
        json!({"expectedRevision": 1, "status": "completed", "openIssues": [], "result": "Done."}),
    )
}

fn shell() -> ResponseEvent {
    function_call("shell", "exec_command", json!({"cmd": "cat README.md"}))
}

fn web_search(status: &str) -> ResponseItem {
    serde_json::from_value(json!({"type": "web_search_call", "id": "search", "status": status}))
        .expect("web search item")
}

fn completed() -> ResponseEvent {
    ResponseEvent::Completed {
        response_id: "response-1".to_string(),
        token_usage: None,
        usage_metadata: None,
        end_turn: None,
    }
}

fn dropped() -> Event {
    Err(ModelResponseError::Stream("dropped".to_string()))
}

fn label(event: &Event) -> String {
    match event {
        Ok(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, .. })) => {
            format!("call:{call_id}")
        }
        Ok(ResponseEvent::OutputItemDone(ResponseItem::WebSearchCall { .. })) => {
            "search:done".to_string()
        }
        Ok(ResponseEvent::OutputItemAdded(ResponseItem::WebSearchCall { .. })) => {
            "search:added".to_string()
        }
        Ok(ResponseEvent::Completed { .. }) => "completed".to_string(),
        Ok(other) => format!("{other:?}"),
        Err(error) => format!("error:{error}"),
    }
}

async fn run_fence(services: &ProjectIntelligenceServices, events: Vec<Event>) -> Vec<String> {
    fence(services, Box::pin(futures::stream::iter(events)))
        .map(|event| label(&event))
        .collect()
        .await
}

#[tokio::test]
async fn a_lone_completion_is_released_after_its_attempt_is_recorded() {
    let fixture = fixture().await;
    assert_eq!(
        run_fence(&fixture.services, vec![Ok(completion()), Ok(completed())]).await,
        vec!["call:complete", "completed"]
    );
    assert_eq!(fixture.counts().await, (0, 1));
}

#[tokio::test]
async fn any_other_call_in_the_response_is_recorded_in_either_order() {
    for (events, expected_counts) in [
        // The sibling is recorded once; the completion flows with it.
        (vec![Ok(completion()), Ok(shell()), Ok(completed())], (1, 0)),
        // Each call is recorded against the bindings current when it arrives.
        (vec![Ok(shell()), Ok(completion()), Ok(completed())], (2, 0)),
        (
            vec![Ok(completion()), Ok(completion()), Ok(completed())],
            (1, 0),
        ),
    ] {
        let fixture = fixture().await;
        let expected = events.iter().map(label).collect::<Vec<_>>();
        assert_eq!(run_fence(&fixture.services, events).await, expected);
        assert_eq!(fixture.counts().await, expected_counts);
    }
}

/// The completion is withheld while its response may still carry other calls; a call after it
/// is recorded and everything flows at once, before the response completes. A call before it
/// is recorded before Core can see it.
#[tokio::test]
async fn calls_reach_core_only_after_their_record() {
    let fixture = fixture().await;
    let (sender, upstream) = channel();
    let mut fenced = fence(&fixture.services, upstream);
    sender.send(Ok(completion())).expect("send");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), fenced.next())
            .await
            .is_err(),
        "a completion is held while its response may still carry other calls"
    );
    sender.send(Ok(shell())).expect("send");
    let first = fenced.next().await.expect("released");
    let second = fenced.next().await.expect("released");
    assert_eq!(
        (label(&first), label(&second), fixture.counts().await),
        (
            "call:complete".to_string(),
            "call:shell".to_string(),
            (1, 0)
        ),
        "recorded and flushed without waiting for the response to complete"
    );

    let fixture = self::fixture().await;
    let (sender, upstream) = channel();
    let mut fenced = fence(&fixture.services, upstream);
    sender.send(Ok(shell())).expect("send");
    let first = fenced.next().await.expect("released");
    assert_eq!(
        (label(&first), fixture.counts().await),
        ("call:shell".to_string(), (1, 0))
    );
}

/// A hosted call is recorded as soon as it is first observed, added or done, before its event
/// passes on, and the record survives the response failing: a reopened store still shows it.
#[tokio::test]
async fn a_hosted_call_is_recorded_when_first_observed_and_survives_failure() {
    let fixture = fixture().await;
    assert_eq!(
        run_fence(
            &fixture.services,
            vec![
                Ok(ResponseEvent::OutputItemAdded(web_search("in_progress"))),
                dropped(),
            ],
        )
        .await,
        vec!["search:added", "error:stream error: dropped"]
    );
    assert_eq!(fixture.counts().await, (1, 0));

    let fixture = self::fixture().await;
    assert_eq!(
        run_fence(
            &fixture.services,
            vec![
                Ok(completion()),
                Ok(ResponseEvent::OutputItemDone(web_search("completed"))),
                dropped(),
            ],
        )
        .await,
        vec![
            "call:complete",
            "search:done",
            "error:stream error: dropped"
        ]
    );
    assert_eq!(fixture.counts().await, (1, 0));
}

/// Records follow the run bindings current at each call, so a run admitted while the response
/// is open is charged for the calls that follow.
#[tokio::test]
async fn a_run_admitted_mid_response_is_charged_for_later_calls() {
    let home = TempDir::new().expect("temporary state home");
    let services = services_at(home.path());
    let (sender, upstream) = channel();
    let mut fenced = fence(&services, upstream);
    sender
        .send(Ok(function_call(
            "plan",
            "update_plan",
            json!({"plan": []}),
        )))
        .expect("send");
    assert_eq!(label(&fenced.next().await.expect("plan")), "call:plan");
    let run_id = create_run(&services, "run-late").await;
    let fixture = Fixture {
        home,
        services,
        run_id,
    };
    sender
        .send(Ok(function_call(
            "plan-2",
            "update_plan",
            json!({"plan": []}),
        )))
        .expect("send");
    sender.send(Ok(completion())).expect("send");
    sender.send(Ok(completed())).expect("send");
    drop(sender);
    let rest = fenced.map(|event| label(&event)).collect::<Vec<_>>().await;
    assert_eq!(rest, vec!["call:plan-2", "call:complete", "completed"]);
    assert_eq!(fixture.counts().await, (2, 0));
}

#[tokio::test]
async fn a_failed_or_unfinished_response_never_releases_a_held_completion() {
    let fixture = fixture().await;
    assert_eq!(
        run_fence(&fixture.services, vec![Ok(completion()), dropped()]).await,
        vec!["error:stream error: dropped"]
    );
    assert_eq!(
        run_fence(&fixture.services, vec![Ok(completion())]).await,
        Vec::<String>::new()
    );
    assert_eq!(fixture.counts().await, (0, 0));
}

/// Dropping the response (cancellation) while a completion is held drops the upstream response
/// too, and the completion never runs.
#[tokio::test]
async fn cancelling_a_held_response_releases_upstream_and_never_the_completion() {
    let fixture = fixture().await;
    let (sender, upstream) = channel();
    let mut fenced = fence(&fixture.services, upstream);
    sender.send(Ok(completion())).expect("send");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), fenced.next())
            .await
            .is_err()
    );
    drop(fenced);
    assert!(sender.is_closed(), "upstream is dropped with the fence");
    assert_eq!(fixture.counts().await, (0, 0));
}

/// Events held behind a completion are bounded in count and bytes. Past either bound the
/// response fails, the completion never runs, and upstream is no longer read.
#[tokio::test]
async fn held_events_are_bounded_in_count_and_bytes() {
    let fixture = fixture().await;
    let pulled = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&pulled);
    let endless_deltas = futures::stream::iter(std::iter::once(Ok(completion())))
        .chain(futures::stream::repeat_with(|| {
            Ok(ResponseEvent::OutputTextDelta(String::new()))
        }))
        .inspect(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
    let labels = fence(&fixture.services, Box::pin(endless_deltas))
        .map(|event| label(&event))
        .collect::<Vec<_>>()
        .await;
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(
        labels[0].contains("exceeded the host's bound"),
        "{labels:?}"
    );
    assert_eq!(pulled.load(Ordering::SeqCst), MAX_HELD_EVENTS + 1);

    let large = "x".repeat(MAX_HELD_BYTES + 1);
    let labels = run_fence(
        &fixture.services,
        vec![
            Ok(completion()),
            Ok(ResponseEvent::OutputTextDelta(large)),
            Ok(completed()),
        ],
    )
    .await;
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(
        labels[0].contains("exceeded the host's bound"),
        "{labels:?}"
    );
    assert_eq!(fixture.counts().await, (0, 0));
}

/// Fault injection: the run store cannot be opened, so no record can be written. The response
/// fails in place of every call, in either position, and in place of a lone completion whose
/// attempt cannot be recorded.
#[tokio::test]
async fn an_unwritable_record_fails_the_response_instead_of_dispatching() {
    let home = TempDir::new().expect("tempdir");
    let not_a_directory = home.path().join("state-file");
    std::fs::write(&not_a_directory, "not a directory").expect("file");
    let services = services_at(&not_a_directory);
    for events in [
        vec![Ok(shell()), Ok(completion()), Ok(completed())],
        vec![Ok(completion()), Ok(shell()), Ok(completed())],
        vec![Ok(completion()), Ok(completed())],
    ] {
        let labels = run_fence(&services, events).await;
        assert_eq!(labels.len(), 1, "{labels:?}");
        assert!(
            labels[0].starts_with(
                "error:stream error: the host could not durably record a model call before dispatch"
            ) && labels[0].ends_with(
                "this call and the following local calls were not dispatched; earlier or provider-hosted actions may already have run"
            ),
            "{labels:?}"
        );
    }
    assert!(super::unrecorded_actions_possible());
}

fn large_completion(bytes: usize) -> ResponseEvent {
    function_call(
        "complete",
        "stateful_run_update",
        json!({"expectedRevision": 1, "status": "completed", "openIssues": [], "result": "x".repeat(bytes)}),
    )
}

fn delta(text: &str) -> Event {
    Ok(ResponseEvent::OutputTextDelta(text.to_string()))
}

/// The bytes an event takes against the bound.
fn size(event: Event) -> usize {
    super::held_size(&event, usize::MAX)
}

/// A completion too large for the bound is never held, released or counted as an attempt,
/// whether the response stays open or completes at once, and the upstream response is
/// dropped.
#[tokio::test]
async fn an_oversized_completion_is_never_held_or_released() {
    let fixture = fixture().await;
    let (sender, upstream) = channel();
    let mut fenced = fence(&fixture.services, upstream);
    sender
        .send(Ok(large_completion(MAX_HELD_BYTES)))
        .expect("send");
    let first = tokio::time::timeout(Duration::from_secs(5), fenced.next())
        .await
        .expect("the response fails at once")
        .expect("an error");
    assert!(
        label(&first).contains("exceeded the host's bound"),
        "{}",
        label(&first)
    );
    assert!(fenced.next().await.is_none());
    assert!(sender.is_closed(), "upstream is dropped");

    let labels = run_fence(
        &fixture.services,
        vec![Ok(large_completion(MAX_HELD_BYTES)), Ok(completed())],
    )
    .await;
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(
        labels[0].contains("exceeded the host's bound"),
        "{labels:?}"
    );
    assert_eq!(fixture.counts().await, (0, 0));
}

/// The event that would release a hold is admitted within the bounds too. At exactly the
/// event or byte bound, the response's `Completed` fails it without a record, and a sibling
/// call fails it after the call is recorded (it was observed); with room for exactly that
/// event, it is released as usual.
#[tokio::test]
async fn the_event_that_releases_a_hold_is_bounded_too() {
    let releasing = |sibling: bool| {
        if sibling {
            Ok(shell())
        } else {
            Ok(completed())
        }
    };
    for sibling in [false, true] {
        for room in [false, true] {
            // Count edge: the completion and text deltas fill the hold, leaving `room`.
            let mut events = vec![Ok(completion())];
            events.extend((1..MAX_HELD_EVENTS - usize::from(room)).map(|_| delta("")));
            events.push(releasing(sibling));
            // Byte edge: one delta fills the bytes, leaving exactly the releasing event's size.
            let spare = if room { size(releasing(sibling)) } else { 0 };
            let text_bytes = MAX_HELD_BYTES - size(Ok(completion())) - size(delta("")) - spare;
            let bytes_events = vec![
                Ok(completion()),
                delta(&"x".repeat(text_bytes)),
                releasing(sibling),
            ];
            for events in [events, bytes_events] {
                let fixture = fixture().await;
                let labels = run_fence(&fixture.services, events).await;
                let case = format!("sibling {sibling}, room {room}");
                if room {
                    assert_eq!(labels[0], "call:complete", "{case}");
                    assert_eq!(
                        labels.last().map(String::as_str),
                        Some(if sibling { "call:shell" } else { "completed" }),
                        "{case}"
                    );
                    let expected = if sibling { (1, 0) } else { (0, 1) };
                    assert_eq!(fixture.counts().await, expected, "{case}");
                } else {
                    assert_eq!(labels.len(), 1, "{case}: {labels:?}");
                    assert!(labels[0].contains("exceeded the host's bound"), "{case}");
                    let expected = if sibling { (1, 0) } else { (0, 0) };
                    assert_eq!(fixture.counts().await, expected, "{case}");
                }
            }
        }
    }
}

/// A sibling waits for its durable record (here blocked by another writer) before anything
/// held is released, and before a sibling that does not fit fails the response: an observed
/// call is accounted before any admission decision.
#[tokio::test]
async fn a_blocked_record_holds_everything_back_even_on_overflow() {
    let fixture = fixture().await;
    let pool = SqliteConfig::new_for_testing(fixture.home.path().abs())
        .open_read_write_pool(&fixture.home.path().join("stateful_runtime_1.sqlite"))
        .await
        .expect("runtime database");

    let writer = pool.begin_with("BEGIN IMMEDIATE").await.expect("writer");
    let (sender, upstream) = channel();
    let mut fenced = fence(&fixture.services, upstream);
    sender.send(Ok(completion())).expect("send");
    sender.send(Ok(shell())).expect("send");
    assert!(
        tokio::time::timeout(Duration::from_millis(300), fenced.next())
            .await
            .is_err(),
        "nothing passes while the sibling's record is blocked"
    );
    writer.rollback().await.expect("unblock");
    let first = fenced.next().await.expect("released");
    let second = fenced.next().await.expect("released");
    assert_eq!(
        (label(&first), label(&second)),
        ("call:complete".to_string(), "call:shell".to_string())
    );
    assert_eq!(fixture.counts().await, (1, 0));

    let fixture = self::fixture().await;
    let pool = SqliteConfig::new_for_testing(fixture.home.path().abs())
        .open_read_write_pool(&fixture.home.path().join("stateful_runtime_1.sqlite"))
        .await
        .expect("runtime database");
    let writer = pool.begin_with("BEGIN IMMEDIATE").await.expect("writer");
    let mut events = vec![Ok(completion())];
    events.extend((1..MAX_HELD_EVENTS).map(|_| delta("")));
    events.push(Ok(shell()));
    let mut fenced = fence(&fixture.services, Box::pin(futures::stream::iter(events)));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), fenced.next())
            .await
            .is_err(),
        "the overflowing sibling is recorded before the response fails"
    );
    writer.rollback().await.expect("unblock");
    let labels = fenced.map(|event| label(&event)).collect::<Vec<_>>().await;
    assert_eq!(labels.len(), 1, "{labels:?}");
    assert!(
        labels[0].contains("exceeded the host's bound"),
        "{labels:?}"
    );
    assert_eq!(fixture.counts().await, (1, 0));
}

/// A hosted call first observed (added or done) when the hold is full by count or by bytes is
/// recorded before the overflow fails the response: the held completion never runs, and a
/// reopened store shows the run is no longer action-free.
#[tokio::test]
async fn a_hosted_call_that_overflows_the_hold_is_still_recorded() {
    let hosted = |added: bool| {
        if added {
            Ok(ResponseEvent::OutputItemAdded(web_search("in_progress")))
        } else {
            Ok(ResponseEvent::OutputItemDone(web_search("completed")))
        }
    };
    for added in [true, false] {
        let mut by_count = vec![Ok(completion())];
        by_count.extend((1..MAX_HELD_EVENTS).map(|_| delta("")));
        by_count.push(hosted(added));
        by_count.push(dropped());
        let text_bytes = MAX_HELD_BYTES - size(Ok(completion())) - size(delta(""));
        let by_bytes = vec![
            Ok(completion()),
            delta(&"x".repeat(text_bytes)),
            hosted(added),
            dropped(),
        ];
        for events in [by_count, by_bytes] {
            let fixture = fixture().await;
            let labels = run_fence(&fixture.services, events).await;
            assert_eq!(labels.len(), 1, "added {added}: {labels:?}");
            assert!(
                labels[0].contains("exceeded the host's bound"),
                "{labels:?}"
            );
            assert_eq!(fixture.counts().await, (1, 0), "added {added}");
        }
    }
}
