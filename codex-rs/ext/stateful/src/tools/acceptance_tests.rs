use std::sync::Arc;

use codex_extension_api::ConversationHistory;
use codex_extension_api::FunctionCallError;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::TurnItem;
use codex_state::SqliteConfig;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RequestSpan;
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

use super::AcceptanceUpdateTool;
use crate::services::ProjectIntelligenceServices;
use crate::tools::run::StatefulRunUpdateTool;
use crate::visible_root::VisibleRootRegistry;

const PROJECT_ID: &str = "project-1";
const THREAD_ID: &str = "thread-1";

struct Fixture {
    _state_home: TempDir,
    services: ProjectIntelligenceServices,
    acceptance: AcceptanceUpdateTool,
    completion: StatefulRunUpdateTool,
    run: StatefulRun,
}

async fn fixture(goal: &str, mode: WorkflowMode) -> Fixture {
    let state_home = TempDir::new().expect("temporary state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let run = services
        .runtime()
        .await
        .expect("runtime opens")
        .create_run(
            StatefulRunId::parse("run-acceptance").expect("valid run ID"),
            NewStatefulRun {
                project_id: PROJECT_ID.to_string(),
                thread_ids: vec![THREAD_ID.to_string()],
                goal: goal.to_string(),
                mode,
                budget: RunBudget {
                    max_continuations: 24,
                    max_elapsed_seconds: 14_400,
                },
            },
        )
        .await
        .expect("run is created");
    Fixture {
        acceptance: AcceptanceUpdateTool::new(
            PROJECT_ID.to_string(),
            THREAD_ID.to_string(),
            services.clone(),
        ),
        completion: StatefulRunUpdateTool::new(
            PROJECT_ID.to_string(),
            THREAD_ID.to_string(),
            services.clone(),
            Arc::new(InMemoryThreadStore::default()),
            /*event_sink*/ None,
            VisibleRootRegistry::default(),
        ),
        _state_home: state_home,
        services,
        run,
    }
}

fn call(tool: &str, arguments: &Value) -> ToolCall<'static> {
    ToolCall {
        turn_id: "turn-1".to_string(),
        call_id: format!("{tool}-call"),
        tool_name: ToolName::plain(tool),
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

impl Fixture {
    async fn update(&self, revision: u64, changes: Value) -> Result<Value, String> {
        match self
            .acceptance
            .handle_call(call(
                "stateful_acceptance_update",
                &json!({"expectedLedgerRevision": revision, "changes": changes}),
            ))
            .await
        {
            Ok(output) => Ok(serde_json::from_str(&output.log_output()).expect("JSON output")),
            Err(FunctionCallError::RespondToModel(message)) => Err(message),
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }

    async fn complete(&self) -> Result<Value, String> {
        let revision = self.stored_run().await.revision;
        match self
            .completion
            .handle(call(
                "stateful_run_update",
                &json!({
                    "expectedRevision": revision,
                    "status": "completed",
                    "completionDisposition": "noReusableLearning",
                    "result": "The parser is fixed.",
                }),
            ))
            .await
        {
            Ok(output) => Ok(serde_json::from_str(&output.log_output()).expect("JSON output")),
            Err(FunctionCallError::RespondToModel(message)) => Err(message),
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }

    async fn stored_run(&self) -> StatefulRun {
        self.services
            .runtime()
            .await
            .expect("runtime opens")
            .get_run(&self.run.id)
            .await
            .expect("run reads")
            .expect("run exists")
    }

    async fn observe(&self, id: &str, script: &str, exit_code: i32) {
        let item: CommandExecutionItem = serde_json::from_value(json!({
            "id": id,
            "command": ["/bin/bash", "-lc", script],
            "cwd": "file:///project",
            "parsed_cmd": [{"type": "unknown", "cmd": script}],
            "source": "agent",
            "status": if exit_code == 0 { "completed" } else { "failed" },
            "aggregated_output": "ok",
            "exit_code": exit_code,
        }))
        .expect("command item decodes");
        crate::acceptance_observation::observe_item(
            &self.services,
            &self.run.id,
            &[],
            &TurnItem::CommandExecution(item),
        )
        .await;
    }
}

#[tokio::test]
async fn user_criteria_link_to_exact_goal_spans() {
    let fixture = fixture(
        "Fix the parser. All tests must pass.",
        WorkflowMode::Autonomous,
    )
    .await;
    let refused = fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "Tests pass.", "requestQuote": "all tests pass", "checkCommand": "pytest -q", "expectedObservation": "every test passes"}]),
        )
        .await
        .expect_err("a paraphrase is not a quote");
    assert!(
        refused.contains("not exact text of the run goal"),
        "{refused}"
    );
    let output = fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes"}]),
        )
        .await
        .expect("criterion added");
    assert_eq!(output["ledgerRevision"], json!(1));
    let ledger = fixture
        .services
        .runtime()
        .await
        .expect("runtime")
        .acceptance_ledger(&fixture.run.id)
        .await
        .expect("ledger");
    assert_eq!(
        ledger.criteria[0].request_span,
        Some(RequestSpan { start: 16, end: 36 })
    );
    assert_eq!(ledger.criteria[0].origin, AcceptanceOrigin::User);
}

#[tokio::test]
async fn completion_runs_the_omission_check_refuses_unmet_gates_and_blocks_after_stalls() {
    let fixture = fixture(
        "Fix the parser. Write the summary to out/report.json. All tests must pass.",
        WorkflowMode::Collaborative,
    )
    .await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes"}]),
        )
        .await
        .expect("criterion added");

    let refused = fixture.complete().await.expect_err("unmet gates refuse");
    for expected in [
        "completion refused; the run stays running (refusal 1 of 3",
        "C1 (All tests pass.): no current host evidence: run exactly `pytest -q`",
        "C2 (Fix the parser.): omission proposal awaiting review",
        "C3 (Write the summary to out/report.json.): omission proposal awaiting review",
    ] {
        assert!(
            refused.contains(expected),
            "{expected} missing from {refused}"
        );
    }
    assert_eq!(
        fixture.stored_run().await.status,
        StatefulRunStatus::Running
    );
    let ledger = fixture
        .services
        .runtime()
        .await
        .expect("runtime")
        .acceptance_ledger(&fixture.run.id)
        .await
        .expect("ledger");
    assert!(ledger.omission_checked);
    assert_eq!(
        ledger
            .criteria
            .iter()
            .map(|criterion| (criterion.origin, criterion.state))
            .collect::<Vec<_>>(),
        vec![
            (AcceptanceOrigin::User, AcceptanceState::Active),
            (AcceptanceOrigin::Omission, AcceptanceState::Proposed),
            (AcceptanceOrigin::Omission, AcceptanceState::Proposed),
        ]
    );

    let second = fixture.complete().await.expect_err("still refused");
    assert!(second.contains("refusal 2 of 3"), "{second}");
    let blocked = fixture.complete().await.expect("host blocks the run");
    assert_eq!(blocked["status"], json!("blocked"));
    let run = fixture.stored_run().await;
    assert_eq!(run.status, StatefulRunStatus::Blocked);
    let result = run.result.expect("partial result recorded");
    assert!(
        result.starts_with("Partial result: completion was refused 3 times"),
        "{result}"
    );
    assert!(result.contains("Last submitted result (not accepted): The parser is fixed."));
}

#[tokio::test]
async fn completion_with_current_evidence_discloses_its_acceptance_basis() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([
                {"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes"},
                {"action": "add", "origin": "derived", "kind": "manual", "statement": "The error message reads well."},
                {"action": "add", "origin": "derived", "kind": "constraint", "statement": "Performance is unchanged.", "required": false}
            ]),
        )
        .await
        .expect("criteria added");
    fixture.observe("check-1", "pytest -q", 0).await;
    let ledger_revision = 2;
    fixture
        .update(
            ledger_revision,
            json!([
                {"action": "observe", "criterion": "C2", "text": "Read the new message in the CLI output."},
                {"action": "noCheck", "criterion": "C3", "text": "No benchmark harness exists in this repository."}
            ]),
        )
        .await
        .expect("labelled evidence recorded");

    let output = fixture.complete().await.expect("completion accepted");
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(
        output["acceptanceBasis"],
        json!([
            "C1 [user] All tests pass.: agent-written check `pytest -q` exited 0 on the host against the final workspace (expected: every test passes)",
            "C2 [derived] The error message reads well.: manual observation (agent-written, not host-verified): Read the new message in the CLI output.",
            "C3 [derived] Performance is unchanged.: OPTIONAL, UNVERIFIED: No benchmark harness exists in this repository."
        ])
    );
    let result = fixture.stored_run().await.result.expect("result stored");
    assert!(
        result.starts_with("The parser is fixed.\n\nAcceptance basis:\n- C1 [user]"),
        "{result}"
    );
}

#[tokio::test]
async fn failed_check_cannot_be_disclosed_away_and_keeps_the_run_running() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes"}]),
        )
        .await
        .expect("criterion added");
    fixture.observe("check-1", "pytest -q", 1).await;
    let refused = fixture
        .update(
            2,
            json!([{"action": "noCheck", "criterion": "C1", "text": "Skipping."}]),
        )
        .await
        .expect_err("a declared check cannot be replaced by noCheck");
    assert!(
        refused.contains("declares the host check `pytest -q`"),
        "{refused}"
    );
    let completion = fixture.complete().await.expect_err("failed check refuses");
    assert!(
        completion.contains("`pytest -q` failed (exit 1)"),
        "{completion}"
    );
    assert_eq!(
        fixture.stored_run().await.status,
        StatefulRunStatus::Running
    );
}

#[tokio::test]
async fn trivial_lookup_completes_without_acceptance_busywork() {
    let fixture = fixture("Answer a lookup.", WorkflowMode::Collaborative).await;
    let output = fixture.complete().await.expect("lookup completes");
    assert_eq!(output.get("acceptanceBasis"), None);
    let ledger = fixture
        .services
        .runtime()
        .await
        .expect("runtime")
        .acceptance_ledger(&fixture.run.id)
        .await
        .expect("ledger");
    assert!(!ledger.omission_checked);
    assert_eq!(
        fixture.stored_run().await.result.as_deref(),
        Some("The parser is fixed.")
    );
}

#[tokio::test]
async fn socratic_pending_run_agrees_criteria_but_records_no_evidence() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Socratic).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "derived", "kind": "manual", "statement": "The plan is agreed."}]),
        )
        .await
        .expect("criteria can be agreed before execution");
    let refused = fixture
        .update(
            1,
            json!([{"action": "observe", "criterion": "C1", "text": "Agreed."}]),
        )
        .await
        .expect_err("no evidence before execution");
    assert!(refused.contains("only while the run executes"), "{refused}");
    let completion = fixture
        .complete()
        .await
        .expect_err("pending runs cannot complete");
    assert!(
        completion.contains("resumed explicitly by the user"),
        "{completion}"
    );
}

#[tokio::test]
async fn required_work_without_a_safe_check_ends_blocked_not_completed() {
    let fixture = fixture("Reconcile the filing.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "manual", "statement": "The filing reconciles.", "requestQuote": "Reconcile the filing."}]),
        )
        .await
        .expect("criterion added");
    fixture
        .update(
            1,
            json!([{"action": "noCheck", "criterion": "C1", "text": "The ledger source is unavailable."}]),
        )
        .await
        .expect("the reason is recorded");
    let refused = fixture
        .complete()
        .await
        .expect_err("unverified required work refuses");
    for expected in [
        "C1 (The filing reconciles.): required but unverified (The ledger source is unavailable.)",
        "set status blocked with a partial result",
    ] {
        assert!(
            refused.contains(expected),
            "{expected} missing from {refused}"
        );
    }
    assert_eq!(
        fixture.stored_run().await.status,
        StatefulRunStatus::Running
    );
    let revision = fixture.stored_run().await.revision;
    let blocked = fixture
        .completion
        .handle(call(
            "stateful_run_update",
            &json!({
                "expectedRevision": revision,
                "status": "blocked",
                "result": "Partial: the filing could not be reconciled because the ledger source is unavailable.",
            }),
        ))
        .await
        .expect("blocked with a partial result");
    let blocked: Value = serde_json::from_str(&blocked.log_output()).expect("JSON output");
    assert_eq!(blocked["status"], json!("blocked"));
    assert_eq!(
        fixture.stored_run().await.status,
        StatefulRunStatus::Blocked
    );
}
