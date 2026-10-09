use std::sync::Arc;

use codex_extension_api::ConversationHistory;
use codex_extension_api::FunctionCallError;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_output_truncation::TruncationPolicy;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::StatefulRunUpdateTool;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

const PROJECT_ID: &str = "project-1";
const THREAD_ID: &str = "thread-1";
const RESULT: &str = "The release gate is build C7-42.";

struct Fixture {
    _state_home: TempDir,
    services: ProjectIntelligenceServices,
    tool: StatefulRunUpdateTool,
    run: StatefulRun,
}

async fn fixture() -> Fixture {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let run = services
        .runtime()
        .await
        .expect("runtime opens")
        .create_run(
            StatefulRunId::parse("run-0123456789").expect("valid run ID"),
            NewStatefulRun {
                project_id: PROJECT_ID.to_string(),
                thread_ids: vec![THREAD_ID.to_string()],
                goal: "Answer a lookup.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run is created");
    let tool = StatefulRunUpdateTool::new(
        PROJECT_ID.to_string(),
        THREAD_ID.to_string(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    Fixture {
        _state_home: state_home,
        services,
        tool,
        run,
    }
}

fn call(arguments: &Value) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: "completion-call".to_string(),
        tool_name: ToolName::plain("stateful_run_update"),
        model: "test-model".to_string(),
        codex_turn_metadata: None,
        truncation_policy: TruncationPolicy::Bytes(20_000),
        source: ToolCallSource::Direct,
        conversation_history: ConversationHistory::default(),
        turn_item_emitter: Arc::new(NoopTurnItemEmitter),
        environments: Vec::new(),
        payload: ToolPayload::Function {
            arguments: arguments.to_string(),
        },
    }
}

fn lookup_completion(expected_revision: u64) -> Value {
    json!({
        "expectedRevision": expected_revision,
        "status": "completed",
        "openIssues": [],
        "completionDisposition": "noReusableLearning",
        "result": RESULT,
    })
}

async fn stored_run(fixture: &Fixture) -> StatefulRun {
    fixture
        .services
        .runtime()
        .await
        .expect("runtime opens")
        .get_run(&fixture.run.id)
        .await
        .expect("run reads")
        .expect("run exists")
}

#[tokio::test]
async fn a_no_tool_lookup_completion_persists_once_without_an_obligation() {
    let fixture = fixture().await;
    // The host recorded no action of the run before this lone completion call, so the lookup
    // completes under the no-tool exemption and says so in its result.
    fixture
        .tool
        .handle_guided(call(&lookup_completion(fixture.run.revision)))
        .await
        .expect("lookup completion succeeds");
    let completed = stored_run(&fixture).await;
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    assert!(
        completed
            .result
            .as_deref()
            .is_some_and(|result| result.starts_with(RESULT)
                && result.contains("no-tool exemption: the host recorded no tool call")),
        "{completed:?}"
    );
    let obligations = fixture
        .services
        .runtime()
        .await
        .expect("runtime opens")
        .list_obligations(
            &fixture.run.id,
            /*after_sequence*/ None,
            /*max_results*/ 10,
        )
        .await
        .expect("obligations list");
    assert_eq!(obligations, Vec::new());
    let repeated = fixture
        .tool
        .handle_guided(call(&lookup_completion(completed.revision)))
        .await;
    assert!(matches!(
        repeated,
        Err(FunctionCallError::RespondToModel(_))
    ));
    assert_eq!(stored_run(&fixture).await, completed);
}

/// Hooks configured in this process could have run around the run's turns and calls (the
/// completion call included), so the lone completion owes the ledger. Nextest runs each test
/// in its own process, so the sticky process fact does not leak into other tests.
#[tokio::test]
async fn a_completion_where_hooks_were_configured_owes_the_ledger() {
    let fixture = fixture().await;
    codex_extension_api::record_host_hooks_configured();
    let refused = fixture
        .tool
        .handle_guided(call(&lookup_completion(fixture.run.revision)))
        .await;
    let Err(FunctionCallError::RespondToModel(message)) = refused else {
        panic!("a completion where hooks could run must be refused");
    };
    assert!(message.contains("completion refused"), "{message}");
    assert_eq!(stored_run(&fixture).await, fixture.run);
}

#[tokio::test]
async fn durable_only_fields_are_rejected_without_mutation() {
    let fixture = fixture().await;
    let contradictory_fields = [
        ("rootRevision", json!(0)),
        ("materialRootFindings", json!([])),
        (
            "materialHistoricalFindings",
            json!([{"entryId": "entry-1", "revision": 1}]),
        ),
        ("completionIdempotencyKey", json!("completion-1")),
        ("finalObligation", json!({"learning": [RESULT]})),
    ];

    for (field, value) in contradictory_fields {
        let mut arguments = lookup_completion(fixture.run.revision);
        arguments[field] = value;

        let rejected = fixture.tool.handle_guided(call(&arguments)).await;

        assert!(
            matches!(rejected, Err(FunctionCallError::RespondToModel(_))),
            "{field} must be rejected"
        );
        assert_eq!(stored_run(&fixture).await, fixture.run, "{field}");
    }
}

#[tokio::test]
async fn stale_revision_cannot_complete_a_lookup() {
    let fixture = fixture().await;

    let rejected = fixture
        .tool
        .handle_guided(call(&lookup_completion(fixture.run.revision + 1)))
        .await;

    assert!(matches!(
        rejected,
        Err(FunctionCallError::RespondToModel(_))
    ));
    assert_eq!(stored_run(&fixture).await, fixture.run);
}

#[tokio::test]
async fn rejected_completions_name_the_field_and_show_a_valid_call() {
    let fixture = fixture().await;
    let revision = fixture.run.revision;
    let valid_lookup = format!(
        r#"{{"expectedRevision":{revision},"status":"completed","completionDisposition":"noReusableLearning","result":"<final answer>","openIssues":[]}}"#
    );
    let mut contradictory = lookup_completion(revision);
    contradictory["rootRevision"] = json!(0);
    contradictory["finalObligation"] = json!({"learning": [RESULT]});
    let mut misspelled = lookup_completion(revision);
    misspelled["status"] = json!("done");

    let mut messages = Vec::new();
    for arguments in [contradictory, misspelled] {
        let Err(FunctionCallError::RespondToModel(message)) =
            fixture.tool.handle_guided(call(&arguments)).await
        else {
            panic!("{arguments} must be rejected");
        };
        messages.push(message);
    }

    assert_eq!(
        messages
            .iter()
            .map(|message| (
                message.contains("remove rootRevision, finalObligation"),
                message.contains("field `status`: unknown variant `done`, expected one of"),
                message.contains(&valid_lookup),
            ))
            .collect::<Vec<_>>(),
        vec![(true, false, true), (false, true, true)],
        "{messages:#?}"
    );
    assert_eq!(stored_run(&fixture).await, fixture.run);
}
