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
use crate::services::ProjectIntelligenceServices;

const THREAD_ID: &str = "thread-1";

struct Fixture {
    _home: TempDir,
    services: ProjectIntelligenceServices,
    run_id: StatefulRunId,
}

async fn fixture() -> Fixture {
    let home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let run_id = StatefulRunId::parse("run-1").expect("run id");
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
    Fixture {
        _home: home,
        services,
        run_id,
    }
}

impl Fixture {
    async fn actions(&self) -> u64 {
        self.services
            .runtime()
            .await
            .expect("runtime")
            .acceptance_ledger(&self.run_id)
            .await
            .expect("ledger")
            .host_actions
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
        provider_executed_tools: false,
    })
    .expect("generation requests are fenced")
    .intercept(upstream)
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

fn completed() -> ResponseEvent {
    ResponseEvent::Completed {
        response_id: "response-1".to_string(),
        token_usage: None,
        usage_metadata: None,
        end_turn: None,
    }
}

fn label(event: &Result<ResponseEvent, ModelResponseError>) -> String {
    match event {
        Ok(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { call_id, .. })) => {
            format!("call:{call_id}")
        }
        Ok(ResponseEvent::Completed { .. }) => "completed".to_string(),
        Ok(other) => format!("{other:?}"),
        Err(error) => format!("error:{error}"),
    }
}

async fn run_fence(
    services: &ProjectIntelligenceServices,
    events: Vec<Result<ResponseEvent, ModelResponseError>>,
) -> Vec<String> {
    fence(services, Box::pin(futures::stream::iter(events)))
        .map(|event| label(&event))
        .collect()
        .await
}

#[tokio::test]
async fn a_lone_completion_passes_unrecorded_after_its_response_completes() {
    let fixture = fixture().await;
    assert_eq!(
        run_fence(&fixture.services, vec![Ok(completion()), Ok(completed())]).await,
        vec!["call:complete", "completed"]
    );
    assert_eq!(fixture.actions().await, 0);
}

#[tokio::test]
async fn any_other_call_in_the_response_is_recorded_in_either_order() {
    for events in [
        vec![Ok(completion()), Ok(shell()), Ok(completed())],
        vec![Ok(shell()), Ok(completion()), Ok(completed())],
        vec![Ok(completion()), Ok(completion()), Ok(completed())],
    ] {
        let fixture = fixture().await;
        let expected = events.iter().map(label).collect::<Vec<_>>();
        assert_eq!(run_fence(&fixture.services, events).await, expected);
        assert_eq!(fixture.actions().await, 1);
    }
}

/// The completion is withheld until its response completes, and a call before it is recorded
/// before Core can see it.
#[tokio::test]
async fn calls_reach_core_only_after_their_record() {
    let fixture = fixture().await;
    let (sender, receiver) = mpsc::unbounded_channel();
    let upstream = futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut fenced = fence(&fixture.services, Box::pin(upstream));

    sender.send(Ok(completion())).expect("send");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), fenced.next())
            .await
            .is_err(),
        "a completion is held while its response may still carry other calls"
    );
    sender.send(Ok(shell())).expect("send");
    sender.send(Ok(completed())).expect("send");
    let first = fenced.next().await.expect("released");
    assert_eq!(label(&first), "call:complete");
    assert_eq!(fixture.actions().await, 1, "recorded before release");

    let fixture = self::fixture().await;
    let (sender, receiver) = mpsc::unbounded_channel();
    let upstream = futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut fenced = fence(&fixture.services, Box::pin(upstream));
    sender.send(Ok(shell())).expect("send");
    let first = fenced.next().await.expect("released");
    assert_eq!(
        (label(&first), fixture.actions().await),
        ("call:shell".to_string(), 1)
    );
}

#[tokio::test]
async fn a_failed_or_unfinished_response_never_releases_a_held_completion() {
    let fixture = fixture().await;
    assert_eq!(
        run_fence(
            &fixture.services,
            vec![
                Ok(completion()),
                Err(ModelResponseError::Stream("dropped".to_string())),
            ],
        )
        .await,
        vec!["error:stream error: dropped"]
    );
    assert_eq!(
        run_fence(&fixture.services, vec![Ok(completion())]).await,
        Vec::<String>::new()
    );
    assert_eq!(fixture.actions().await, 0);
}

/// Fault injection: the run store cannot be opened, so no record can be written. The response
/// fails in place of every call, in either position.
#[tokio::test]
async fn an_unwritable_record_fails_the_response_instead_of_dispatching() {
    let home = TempDir::new().expect("tempdir");
    let not_a_directory = home.path().join("state-file");
    std::fs::write(&not_a_directory, "not a directory").expect("file");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(not_a_directory.abs()));
    for events in [
        vec![Ok(shell()), Ok(completion()), Ok(completed())],
        vec![Ok(completion()), Ok(shell()), Ok(completed())],
    ] {
        let labels = run_fence(&services, events).await;
        assert_eq!(labels.len(), 1, "{labels:?}");
        assert!(
            labels[0].starts_with("error:stream error: the host could not durably record"),
            "{labels:?}"
        );
    }
    // A text-only response with a lone completion needs no record and still passes.
    assert_eq!(
        run_fence(&services, vec![Ok(completion()), Ok(completed())]).await,
        vec!["call:complete", "completed"]
    );
}
