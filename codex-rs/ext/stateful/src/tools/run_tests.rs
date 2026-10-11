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
    fixture_for("Answer a lookup.", WorkflowMode::Collaborative).await
}

async fn fixture_for(goal: &str, mode: WorkflowMode) -> Fixture {
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
                goal: goal.to_string(),
                mode,
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
async fn a_criterion_free_lookup_completion_is_refused() {
    let fixture = fixture().await;
    // No request is exempt from acceptance: without a covering criterion and a current
    // receipt of its admitted check plan, the run stays running and nothing is persisted.
    let refused = fixture
        .tool
        .handle_guided(call(&lookup_completion(fixture.run.revision)))
        .await;
    let Err(FunctionCallError::RespondToModel(message)) = refused else {
        panic!("a criterion-free completion must be refused");
    };
    assert!(message.contains("completion refused"), "{message}");
    assert_eq!(stored_run(&fixture).await, fixture.run);
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

/// E1d2 verification: a Collaborative run has no host-initiated route to Blocked, but the
/// model's own `stateful_run_update` (rostered in every mode) reaches it three ways even when
/// the run is a plain question: a direct blocked update, a completion declaring an open issue,
/// and the host escalation after `MAX_STALLED_COMPLETIONS` refused completions.
#[tokio::test]
async fn a_collaborative_plain_question_can_still_end_blocked_through_run_update() {
    const QUESTION: &str = "What is the capital of France?";
    let mut statuses = Vec::new();

    let direct = fixture_for(QUESTION, WorkflowMode::Collaborative).await;
    let blocked = json!({
        "expectedRevision": direct.run.revision,
        "status": "blocked",
        "result": "Paris.",
    });
    direct
        .tool
        .handle_guided(call(&blocked))
        .await
        .expect("a direct blocked update is accepted");
    statuses.push(stored_run(&direct).await.status);

    let open_issue = fixture_for(QUESTION, WorkflowMode::Collaborative).await;
    let mut completion = lookup_completion(open_issue.run.revision);
    completion["openIssues"] = json!(["The answer was not checked."]);
    open_issue
        .tool
        .handle_guided(call(&completion))
        .await
        .expect("a completion with an open issue ends blocked");
    statuses.push(stored_run(&open_issue).await.status);

    let refused = fixture_for(QUESTION, WorkflowMode::Collaborative).await;
    for attempt in 1..=super::run_acceptance::MAX_STALLED_COMPLETIONS {
        let revision = stored_run(&refused).await.revision;
        let outcome = refused
            .tool
            .handle_guided(call(&lookup_completion(revision)))
            .await;
        assert_eq!(
            outcome.is_ok(),
            attempt == super::run_acceptance::MAX_STALLED_COMPLETIONS,
            "attempt {attempt}"
        );
    }
    statuses.push(stored_run(&refused).await.status);

    assert_eq!(
        (refused.run.value.mode, statuses),
        (
            WorkflowMode::Collaborative,
            vec![
                StatefulRunStatus::Blocked,
                StatefulRunStatus::Blocked,
                StatefulRunStatus::Blocked,
            ]
        )
    );
}

/// An Autonomous run's refused completion tells the model that a plain question needs no
/// acceptance work and ends answered, never blocked; other modes get the plain refusal.
#[tokio::test]
async fn an_autonomous_refusal_points_a_plain_question_at_the_answered_outcome() {
    const QUESTION: &str = "What is the capital of France?";
    let mut refusals = Vec::new();
    for mode in [WorkflowMode::Autonomous, WorkflowMode::Collaborative] {
        let fixture = fixture_for(QUESTION, mode).await;
        let decision = fixture
            .tool
            .acceptance_decision(
                &fixture.run,
                Some("Paris."),
                /*local_executor*/ true,
                /*validated_obligation_sequence*/ None,
            )
            .await
            .expect("the gate decides");
        let super::run_acceptance::AcceptanceDecision::Refused(message) = decision else {
            panic!("a criterion-free completion is refused");
        };
        refusals.push(message);
    }
    let refusal = "completion refused; the run stays running (refusal 1 of 3 without acceptance progress before the host blocks it as partial). Unmet acceptance gates: C1 (What is the capital of France?): omission proposal awaiting review: accept it as a criterion, or dismiss it only as coveredBy a user criterion. Settle each gate: repair the work and run its exact checkCommand with the shell tool, accept each omission proposal (dismiss only as covered by a user criterion), give each check its checker files and have the host admit its plan (admit). noCheck settles only optional criteria. Required work never completes unverified: if it cannot be verified or finished, set status blocked with a partial result that names the unmet criteria. Otherwise complete again";
    assert_eq!(
        refusals,
        vec![
            format!(
                "{refusal}. If the request is a plain question that needs no work, it needs no acceptance work either: do not call stateful_run_update again and do not set blocked for it; give the answer as your final message, ending with the four lines [stateful-outcome], disposition: answer, open-issues: none, [/stateful-outcome], and the run ends answered, not verified"
            ),
            refusal.to_string(),
        ]
    );
}
