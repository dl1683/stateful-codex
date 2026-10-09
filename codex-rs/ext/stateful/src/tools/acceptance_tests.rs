use std::sync::Arc;

use codex_extension_api::ConversationHistory;
use codex_extension_api::FunctionCallError;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_state::SqliteConfig;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::CommandEvidence;
use codex_stateful_runtime::EvidenceOutcome;
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
            Arc::new(InMemoryThreadStore::default()),
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
    /// The host recorded an action of the run (a tool call), so the no-tool exemption no
    /// longer applies.
    async fn action(&self) {
        self.services
            .runtime()
            .await
            .expect("runtime")
            .record_host_action(&self.run.id)
            .await
            .expect("action");
    }

    async fn update(&self, revision: u64, changes: Value) -> Result<Value, String> {
        match self
            .acceptance
            .handle_bounded(call(
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
                    "openIssues": [],
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

    async fn record_check(&self, ordinal: u32, exit_code: i32) {
        let runtime = self.services.runtime().await.expect("runtime");
        let ledger = runtime
            .acceptance_ledger(&self.run.id)
            .await
            .expect("ledger");
        let criterion = ledger.criterion(ordinal).expect("criterion");
        runtime
            .record_command_evidence(
                &self.run.id,
                vec![CommandEvidence {
                    ordinal,
                    criterion_revision: criterion.revision,
                    outcome: if exit_code == 0 {
                        EvidenceOutcome::Passed
                    } else {
                        EvidenceOutcome::Failed
                    },
                    command: criterion.check_command.clone().expect("check"),
                    exit_code: Some(exit_code),
                    output_tail: "ok".to_string(),
                    output_digest: "sha256:output".to_string(),
                    artifact_digest: Some("sha256:pinned".to_string()),
                    checker_digest: None,
                    detail: None,
                    start_generation: ledger.workspace_generation,
                    source_id: "call-check".to_string(),
                }],
            )
            .await
            .expect("evidence recorded");
    }

    async fn ledger(&self) -> codex_stateful_runtime::AcceptanceLedger {
        self.services
            .runtime()
            .await
            .expect("runtime")
            .acceptance_ledger(&self.run.id)
            .await
            .expect("ledger")
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
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "Tests pass.", "requestQuote": "all tests pass", "checkCommand": "pytest -q", "expectedObservation": "every test passes", "artifacts": ["tests/test_parser.py"]}]),
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
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes", "artifacts": ["tests/test_parser.py"]}]),
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
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes", "artifacts": ["tests/test_parser.py"]}]),
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
async fn superficial_or_foreign_executor_receipts_are_observational() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "echo suite-ok", "expectedObservation": "the suite reports success", "artifacts": ["out.txt"], "checker": ["tests/test_parser.py"]}]),
        )
        .await
        .expect("criterion added");
    // An echo executes no checker file, so the host refuses to admit it as the method.
    let superficial = fixture
        .update(1, json!([{"action": "admit", "criterion": "C1"}]))
        .await
        .expect_err("an echo is no check of its checker");
    assert!(
        superficial.contains("does not execute one of C1's checker files"),
        "{superficial}"
    );
    fixture.record_check(1, 0).await;
    // The unit-test call carries no local environment, so no file evidence qualifies.
    let refused = fixture
        .complete()
        .await
        .expect_err("a passing echo settles nothing");
    assert!(
        refused.contains("executor is not the local host, so file evidence is unsupported"),
        "{refused}"
    );
    assert_eq!(
        fixture.stored_run().await.status,
        StatefulRunStatus::Running
    );
}

#[tokio::test]
async fn admission_outside_the_local_executor_cannot_freeze_checker_bytes() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Collaborative).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "python3 tests/test_parser.py", "expectedObservation": "every test passes", "artifacts": ["src/parser.py"], "checker": ["tests/test_parser.py"]}]),
        )
        .await
        .expect("criterion added");
    let refused = fixture
        .update(1, json!([{"action": "admit", "criterion": "C1"}]))
        .await
        .expect_err("no local executor");
    assert!(
        refused.contains("could not be read on the local executor"),
        "{refused}"
    );
}

#[tokio::test]
async fn identical_no_check_statements_do_not_reset_the_refusal_count() {
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
            json!([{"action": "noCheck", "criterion": "C1", "text": "No source."}]),
        )
        .await
        .expect("first statement");
    for attempt in 1..=2 {
        let refused = fixture.complete().await.expect_err("unmet");
        assert!(
            refused.contains(&format!("refusal {attempt} of 3")),
            "{refused}"
        );
        fixture
            .update(
                2,
                json!([{"action": "noCheck", "criterion": "C1", "text": "No source."}]),
            )
            .await
            .expect("identical restatement is accepted without progress");
    }
    let blocked = fixture
        .complete()
        .await
        .expect("third unchanged refusal blocks");
    assert_eq!(blocked["status"], json!("blocked"));
}
#[tokio::test]
async fn a_dismissal_without_a_covering_criterion_is_refused() {
    let fixture = fixture(
        "Fix the parser. Write the report.",
        WorkflowMode::Autonomous,
    )
    .await;
    fixture.action().await;
    fixture.complete().await.expect_err("proposals created");
    let ledger = fixture.ledger().await;
    let refused = fixture
        .update(
            ledger.revision,
            json!([{"action": "dismiss", "criterion": "C2", "text": "The user withdrew the report in steering."}]),
        )
        .await
        .expect_err("model prose is no receipt");
    assert!(refused.contains("only as coveredBy"), "{refused}");
    let quoted = fixture
        .update(
            ledger.revision,
            json!([{"action": "dismiss", "criterion": "C2", "text": "Withdrawn.", "steeringId": "made-up"}]),
        )
        .await
        .expect_err("a steering reference is no receipt");
    assert!(quoted.contains("only as coveredBy"), "{quoted}");
    assert_eq!(fixture.ledger().await, ledger);
}
#[tokio::test]
async fn acceptance_refusals_fit_the_call_allowance_and_dependency_bounds_hold() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Autonomous).await;
    let mut tiny = call(
        "stateful_acceptance_update",
        &json!({"expectedLedgerRevision": 0, "changes": [{"action": "refine", "criterion": format!("X{}", "y".repeat(12_000))}]}),
    );
    tiny.truncation_policy = TruncationPolicy::Bytes(200);
    let budget = tiny.response_byte_budget(crate::tools::MAX_RESPONSE_BYTES);
    match fixture.acceptance.handle_bounded(tiny).await {
        Err(FunctionCallError::RespondToModel(message)) => {
            assert!(
                serde_json::to_string(&message).expect("serializes").len() <= budget,
                "{message}"
            );
        }
        other => panic!("expected a bounded refusal, got {:?}", other.is_ok()),
    }
    fixture
        .update(0, json!([{"action": "add", "origin": "derived", "kind": "manual", "statement": "First."}]))
        .await
        .expect("C1");
    let dependencies = vec!["C1"; 9];
    let refused = fixture
        .update(
            1,
            json!([{"action": "add", "origin": "derived", "kind": "manual", "statement": "Second.", "dependsOn": dependencies}]),
        )
        .await
        .expect_err("oversized dependency vector");
    assert!(refused.contains("dependsOn names at most 8"), "{refused}");
    let refused = fixture
        .update(
            1,
            json!([{"action": "add", "origin": "derived", "kind": "manual", "statement": "Second.", "dependsOn": ["C1", "C1"]}]),
        )
        .await
        .expect_err("duplicate dependencies");
    assert!(refused.contains("distinct earlier criteria"), "{refused}");
}

#[tokio::test]
async fn no_op_refinements_do_not_reset_the_refusal_count() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes", "artifacts": ["tests/test_parser.py"]}]),
        )
        .await
        .expect("criterion added");
    for attempt in 1..=2 {
        let refused = fixture.complete().await.expect_err("unmet");
        assert!(
            refused.contains(&format!("refusal {attempt} of 3")),
            "{refused}"
        );
        fixture
            .update(1, json!([{"action": "refine", "criterion": "C1"}]))
            .await
            .expect("no-op refinement");
    }
    let blocked = fixture
        .complete()
        .await
        .expect("third unchanged refusal blocks");
    assert_eq!(blocked["status"], json!("blocked"));
}

#[tokio::test]
async fn failed_check_cannot_be_disclosed_away_and_keeps_the_run_running() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Autonomous).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "user", "kind": "check", "statement": "All tests pass.", "requestQuote": "All tests must pass.", "checkCommand": "pytest -q", "expectedObservation": "every test passes", "artifacts": ["tests/test_parser.py"]}]),
        )
        .await
        .expect("criterion added");
    fixture.record_check(1, 1).await;
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
async fn a_no_tool_lookup_completes_and_a_recorded_action_brings_the_ledger_back() {
    let exempt = fixture("Answer a lookup.", WorkflowMode::Collaborative).await;
    // The response fence records the lone completion's attempt before releasing it.
    exempt
        .services
        .runtime()
        .await
        .expect("runtime")
        .record_completion_attempt_for_thread(THREAD_ID)
        .await
        .expect("attempt");
    exempt
        .complete()
        .await
        .expect("a run without any action completes");
    crate::exempt_completion::finish_turn(&exempt.services, /*event_sink*/ None, "turn-1").await;
    assert_eq!(
        exempt.stored_run().await.status,
        StatefulRunStatus::Completed
    );
    assert!(exempt.ledger().await.no_tool_exemption);

    let effected = fixture("Answer a lookup.", WorkflowMode::Collaborative).await;
    effected.action().await;
    let refused = effected
        .complete()
        .await
        .expect_err("an action requires the ledger");
    assert!(refused.contains("Answer a lookup."), "{refused}");
    assert_eq!(
        effected.stored_run().await.status,
        StatefulRunStatus::Running
    );
}

#[tokio::test]
async fn socratic_pending_run_agrees_criteria_but_records_no_evidence() {
    let fixture = fixture("All tests must pass.", WorkflowMode::Socratic).await;
    fixture
        .update(
            0,
            json!([{"action": "add", "origin": "derived", "kind": "manual", "statement": "The plan is agreed.", "artifacts": ["PLAN.md"]}]),
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

impl Fixture {
    async fn complete_with(&self, arguments: Value) -> Result<Value, String> {
        match self
            .completion
            .handle(call("stateful_run_update", &arguments))
            .await
        {
            Ok(output) => Ok(serde_json::from_str(&output.log_output()).expect("JSON output")),
            Err(FunctionCallError::RespondToModel(message)) => Err(message),
            Err(error) => panic!("unexpected error: {error:?}"),
        }
    }
}

#[tokio::test]
async fn an_admitted_open_issue_ends_the_run_blocked_not_completed() {
    let fixture = fixture("Answer a lookup.", WorkflowMode::Autonomous).await;
    let revision = fixture.stored_run().await.revision;
    let missing = fixture
        .complete_with(json!({
            "expectedRevision": revision,
            "status": "completed",
            "completionDisposition": "noReusableLearning",
            "result": "The answer is 42.",
        }))
        .await
        .expect_err("openIssues is required with completed");
    assert!(
        missing.contains("completed requires openIssues"),
        "{missing}"
    );
    assert_eq!(
        fixture.stored_run().await.status,
        StatefulRunStatus::Running
    );

    let blocked = fixture
        .complete_with(json!({
            "expectedRevision": revision,
            "status": "completed",
            "completionDisposition": "noReusableLearning",
            "result": "The answer is 42.",
            "openIssues": ["The second source gives 41; the discrepancy is unresolved."],
        }))
        .await
        .expect("the host records the admitted issue");
    assert_eq!(blocked["status"], json!("blocked"));
    let run = fixture.stored_run().await;
    assert_eq!(run.status, StatefulRunStatus::Blocked);
    assert_eq!(
        run.result.as_deref(),
        Some(
            "Partial result: the completion declared unresolved issues, so the run is blocked instead of completed.\nOpen issues:\n- The second source gives 41; the discrepancy is unresolved.\n\nSubmitted result (not accepted as complete): The answer is 42."
        )
    );
}

#[tokio::test]
async fn a_recorded_open_blocker_ends_the_run_blocked_not_completed() {
    let fixture = fixture("Answer a lookup.", WorkflowMode::Collaborative).await;
    fixture
        .services
        .runtime()
        .await
        .expect("runtime")
        .append_obligation(
            "obligation-blocker".to_string(),
            codex_stateful_runtime::NewObligation {
                project_id: PROJECT_ID.to_string(),
                run_id: fixture.run.id.clone(),
                packet: codex_stateful_runtime::ObligationPacket {
                    blockers: vec!["The verifier fixture is missing.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "call-obligation".to_string(),
            },
        )
        .await
        .expect("obligation recorded");
    let blocked = fixture
        .complete()
        .await
        .expect("the host records the blocker");
    assert_eq!(blocked["status"], json!("blocked"));
    let run = fixture.stored_run().await;
    assert_eq!(run.status, StatefulRunStatus::Blocked);
    assert!(run.result.as_deref().is_some_and(|result| {
        result.contains("- Recorded blocker: The verifier fixture is missing.")
    }));
}

#[tokio::test]
async fn a_long_request_with_budget_left_requires_the_independent_omission_check() {
    let fixture = fixture(
        "Convert every invoice in data/ to the new schema. Write the totals to out/totals.csv. Keep the original files unchanged.",
        WorkflowMode::Autonomous,
    )
    .await;
    // Nothing declared, a tool call recorded, nearly the whole budget left: the agent still
    // cannot complete before the host's goal-derived check has been reviewed.
    fixture.action().await;
    let refused = fixture
        .complete()
        .await
        .expect_err("the omission check is required");
    for expected in [
        "C1 (Convert every invoice in data/ to the new schema.): omission proposal awaiting review",
        "C2 (Write the totals to out/totals.csv.): omission proposal awaiting review",
        "C3 (Keep the original files unchanged.): omission proposal awaiting review",
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
}
