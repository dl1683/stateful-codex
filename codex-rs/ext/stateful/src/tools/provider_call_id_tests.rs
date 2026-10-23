use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_project_intelligence::BlackboardEntryId;
use codex_state::SqliteConfig;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::WorkflowMode;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::blackboard_write::BlackboardBatchRecordTool;
use super::capture_repair_tests::call;
use super::obligation::ObligationUpdateTool;
use super::provenance_source_id;
use super::stable_id;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRootRegistry;

const PROJECT_ID: &str = "project-1";
const THREAD_ID: &str = "thread-1";

/// A Responses bridge call ID that carries a Gemini thought signature, as LiteLLM emits it.
fn bridged_call_id() -> String {
    format!(
        "call_3f1c9a7e2b__thought__{}",
        "Q2lZQjQ0a2Z".repeat(/*n*/ 200)
    )
}

fn with_call_id(tool: &str, arguments: Value, call_id: &str) -> ToolCall<'static> {
    let mut call = call(
        tool,
        arguments,
        /*budget*/ 20_000,
        ToolCallSource::Direct,
    );
    call.call_id = call_id.to_string();
    call
}

struct Fixture {
    _state_home: TempDir,
    services: ProjectIntelligenceServices,
    run: StatefulRun,
    records: BlackboardBatchRecordTool,
    obligations: ObligationUpdateTool,
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
                goal: "Fix the calculator.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run is created");
    Fixture {
        records: BlackboardBatchRecordTool::new(
            PROJECT_ID.to_string(),
            THREAD_ID.to_string(),
            services.clone(),
            Arc::new(InMemoryThreadStore::default()),
            /*event_sink*/ None,
            VisibleRootRegistry::default(),
        ),
        obligations: ObligationUpdateTool::new(
            PROJECT_ID.to_string(),
            THREAD_ID.to_string(),
            services.clone(),
            /*event_sink*/ None,
            Default::default(),
        ),
        _state_home: state_home,
        services,
        run,
    }
}

fn finding(key: &str) -> Value {
    json!({
        "idempotencyKey": key,
        "kind": "fact",
        "content": format!("calc.add finding {key}."),
        "confidenceBasisPoints": 9000,
        "verification": "unverified",
        "importance": "normal",
        "rootPromotion": "notPromoted"
    })
}

fn batch() -> Value {
    json!({
        "records": [finding("finding-1"), finding("finding-2")],
        "relations": [{
            "idempotencyKey": "relation-1",
            "fromRecordKey": "finding-1",
            "toRecordKey": "finding-2",
            "kind": "supports",
            "confidenceBasisPoints": 8000
        }]
    })
}

fn progress() -> Value {
    json!({
        "idempotencyKey": "progress-1",
        "packet": {"learning": ["calc.add subtracted"], "next": ["fix calc.add"]}
    })
}

impl Fixture {
    async fn record(&self, call_id: &str) -> Result<Value, String> {
        match self
            .records
            .handle(with_call_id("blackboard_record_batch", batch(), call_id))
            .await
        {
            Ok(output) => Ok(serde_json::from_str(&output.log_output()).expect("JSON output")),
            Err(error) => Err(error.to_string()),
        }
    }

    async fn update(&self, call_id: &str) -> Result<(), FunctionCallError> {
        self.obligations
            .handle(with_call_id("obligation_update", progress(), call_id))
            .await
            .map(drop)
    }

    /// Each stored finding's provenance source and revision.
    async fn findings(&self) -> Vec<Option<(String, u64)>> {
        let store = self.services.blackboard().await.expect("blackboard opens");
        let mut findings = Vec::new();
        for key in ["finding-1", "finding-2"] {
            let id = BlackboardEntryId::parse(stable_id("entry", PROJECT_ID, key))
                .expect("valid entry ID");
            findings.push(
                store
                    .get_entry(PROJECT_ID, &id)
                    .await
                    .expect("entry reads")
                    .map(|entry| (entry.value.provenance.source_id, entry.revision)),
            );
        }
        findings
    }

    async fn obligation_sources(&self) -> Vec<String> {
        self.services
            .runtime()
            .await
            .expect("runtime opens")
            .list_obligations(
                &self.run.id,
                /*after_sequence*/ None,
                /*max_results*/ 10,
            )
            .await
            .expect("obligations list")
            .into_iter()
            .map(|obligation| obligation.value.provenance_source_id)
            .collect()
    }
}

fn counts(output: &Value) -> (Value, Value, Value, Value) {
    (
        output["recorded"].clone(),
        output["failed"].clone(),
        output["relationsRecorded"].clone(),
        output["relationsFailed"].clone(),
    )
}

#[test]
fn provenance_is_bounded_deterministic_and_uses_the_whole_id() {
    let source = |raw: &str| crate::call_source::call_source(raw).expect("nonempty");
    for raw in ["c".repeat(/*n*/ 512), "é".repeat(/*n*/ 256)] {
        assert_eq!(source(&raw), raw);
    }
    for raw in [
        "c".repeat(/*n*/ 513),
        format!("{}x", "é".repeat(/*n*/ 256)),
        "é".repeat(/*n*/ 257),
        bridged_call_id(),
        "call\n1".to_string(),
        " padded ".to_string(),
        "trailing ".to_string(),
        "call\0id".to_string(),
        format!("stateful:call:v1:sha256:{}", "0".repeat(/*n*/ 64)),
    ] {
        let mapped = source(&raw);
        assert_eq!(mapped.len(), 88);
        assert_ne!(mapped, raw);
        assert_eq!(mapped, source(&raw));
        assert_ne!(mapped, source(&format!("{raw}x")));
        assert!(
            mapped
                .strip_prefix("stateful:call:v1:sha256:")
                .expect("prefix")
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }
}

/// IDs the stores accepted verbatim before (a 200-byte ID, and one that looks like a
/// namespace earlier drafts reserved) still match their stored records, relations and
/// obligations on an exact retry.
#[tokio::test]
async fn store_valid_ids_replay_records_relations_and_obligations_unchanged() {
    for call_id in [
        format!("call_{}", "7".repeat(/*n*/ 195)),
        "stateful:oversized-call-id:v1:sha256:legacy".to_string(),
    ] {
        let fixture = fixture().await;

        let first = fixture
            .record(&call_id)
            .await
            .expect("the batch is accepted");
        let replay = fixture
            .record(&call_id)
            .await
            .expect("the retry is accepted");
        fixture.update(&call_id).await.expect("obligation recorded");
        fixture.update(&call_id).await.expect("obligation retry");

        let expected = (json!(2), json!(0), json!(1), json!(0));
        assert_eq!(
            (
                counts(&first),
                counts(&replay),
                fixture.findings().await,
                fixture.obligation_sources().await
            ),
            (
                expected.clone(),
                expected,
                vec![Some((call_id.clone(), 1)), Some((call_id.clone(), 1))],
                vec![call_id],
            )
        );
    }
}

/// An empty identity is still refused before any write.
#[tokio::test]
async fn unrecordable_call_ids_write_nothing() {
    let fixture = fixture().await;
    let before = fixture
        .services
        .blackboard()
        .await
        .expect("blackboard opens")
        .project_revision(PROJECT_ID)
        .await
        .expect("revision reads");

    let mut refusals = Vec::new();
    for call_id in [""] {
        refusals.push((
            fixture.record(call_id).await.is_err(),
            matches!(
                fixture.update(call_id).await,
                Err(FunctionCallError::RespondToModel(_))
            ),
        ));
    }

    assert_eq!(
        (
            refusals,
            fixture
                .services
                .blackboard()
                .await
                .expect("blackboard opens")
                .project_revision(PROJECT_ID)
                .await
                .expect("revision reads"),
            fixture.findings().await,
            fixture.obligation_sources().await,
        ),
        (vec![(true, true)], before, vec![None, None], Vec::new())
    );
}

#[tokio::test]
async fn oversized_sources_record_batches_relations_and_obligations() {
    for size in [539, 67_222] {
        let fixture = fixture().await;
        let raw = "x".repeat(size);
        let source = provenance_source_id(&with_call_id("obligation_update", json!({}), &raw))
            .expect("mapped");
        assert_eq!(
            counts(&fixture.record(&raw).await.expect("batch")),
            (json!(2), json!(0), json!(1), json!(0))
        );
        let mut different_tail = raw.clone();
        different_tail.pop();
        different_tail.push('y');
        assert_ne!(
            source,
            provenance_source_id(&with_call_id(
                "obligation_update",
                json!({}),
                &different_tail
            ))
            .unwrap()
        );
        fixture.update(&raw).await.expect("obligation");
        fixture.update(&raw).await.expect("retry");
        assert_eq!(
            (fixture.findings().await, fixture.obligation_sources().await),
            (
                vec![Some((source.clone(), 1)), Some((source.clone(), 1))],
                vec![source]
            )
        );
    }
}

#[tokio::test]
async fn historical_reserved_sources_replay_without_rewriting() {
    let raw = "stateful:call:v1:sha256:historical";
    let seed = fixture().await;
    seed.record("seed").await.expect("seed batch");
    seed.update("seed").await.expect("seed obligation");
    let target = fixture().await;
    let node = target
        .services
        .project_node_id(PROJECT_ID)
        .await
        .expect("node");
    let store = target.services.blackboard().await.expect("store");
    let seed_store = seed.services.blackboard().await.expect("seed store");
    let first_id = BlackboardEntryId::parse(stable_id("entry", PROJECT_ID, "finding-1")).unwrap();
    for key in ["finding-1", "finding-2"] {
        let id = BlackboardEntryId::parse(stable_id("entry", PROJECT_ID, key)).unwrap();
        let mut value = seed_store
            .get_entry(PROJECT_ID, &id)
            .await
            .unwrap()
            .unwrap()
            .value;
        value.node_id = node.clone();
        value.provenance.source_id = raw.to_string();
        store
            .create_entry(id, value)
            .await
            .expect("historical entry");
    }
    for relation in seed_store
        .list_relations(PROJECT_ID, &first_id, /*max_results*/ 256)
        .await
        .unwrap()
    {
        let mut value = relation.value;
        value.provenance.source_id = raw.to_string();
        store
            .create_relation(relation.id, value)
            .await
            .expect("historical relation");
    }
    let runtime = target.services.runtime().await.unwrap();
    let mut obligation = seed
        .services
        .runtime()
        .await
        .unwrap()
        .latest_obligation(&seed.run.id)
        .await
        .unwrap()
        .unwrap();
    obligation.value.provenance_source_id = raw.to_string();
    runtime
        .append_obligation(obligation.id, obligation.value)
        .await
        .expect("historical obligation");
    assert_eq!(
        counts(&target.record(raw).await.expect("historical retry")),
        (json!(2), json!(0), json!(1), json!(0))
    );
    target
        .update(raw)
        .await
        .expect("historical obligation retry");
    assert_eq!(
        (target.findings().await, target.obligation_sources().await),
        (vec![Some((raw.to_string(), 1)); 2], vec![raw.to_string()])
    );
    let relation_tool = super::blackboard_write::BlackboardRelateTool::new(
        PROJECT_ID.to_string(),
        target.services.clone(),
        /*event_sink*/ None,
    );
    relation_tool
        .handle(with_call_id(
            "blackboard_relate",
            json!({
                "idempotencyKey": "relation-1", "fromEntryId": first_id.to_string(),
                "toEntryId": stable_id("entry", PROJECT_ID, "finding-2"),
                "kind": "supports", "confidenceBasisPoints": 8000
            }),
            raw,
        ))
        .await
        .expect("historical standalone retry");
    assert_eq!(
        store
            .list_relations(PROJECT_ID, &first_id, /*max_results*/ 256)
            .await
            .unwrap()
            .into_iter()
            .map(|relation| (relation.value.provenance.source_id, relation.revision))
            .collect::<Vec<_>>(),
        vec![(raw.to_string(), 1)]
    );
    let mut changed = progress();
    changed["packet"]["learning"] = json!(["different payload"]);
    assert!(
        target
            .obligations
            .handle(with_call_id("obligation_update", changed, raw))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn oversized_ids_reach_acceptance_and_completion_validation() {
    for size in [539, 67_222] {
        let fixture = fixture().await;
        let raw = "x".repeat(size);
        let acceptance = super::acceptance::AcceptanceUpdateTool::new(
            PROJECT_ID.to_string(),
            THREAD_ID.to_string(),
            fixture.services.clone(),
            Arc::new(InMemoryThreadStore::default()),
        );
        acceptance.handle(with_call_id("stateful_acceptance_update", json!({
            "expectedLedgerRevision": 0,
            "changes": [{"action": "add", "origin": "derived", "kind": "check", "statement": "Tests pass.",
                "checkCommand": "pytest", "expectedObservation": "exit 0", "artifacts": ["out.txt"]}]
        }), &raw)).await.expect("oversized acceptance ID accepted");
        let completion = super::run::StatefulRunUpdateTool::new(
            PROJECT_ID.to_string(),
            THREAD_ID.to_string(),
            fixture.services.clone(),
            Arc::new(InMemoryThreadStore::default()),
            /*event_sink*/ None,
            VisibleRootRegistry::default(),
            crate::deliverable::TurnRunCommits::default(),
        );
        let result = completion.handle(with_call_id("stateful_run_update", json!({
            "expectedRevision": fixture.run.revision, "status": "completed", "openIssues": [],
            "result": "Fixed."
        }), &raw)).await;
        let Err(FunctionCallError::RespondToModel(message)) = result else {
            panic!("missing completion fields");
        };
        assert!(
            message.contains("completed requires materialRootFindings"),
            "{message}"
        );
        let relation = super::blackboard_write::BlackboardRelateTool::new(
            PROJECT_ID.to_string(),
            fixture.services.clone(),
            /*event_sink*/ None,
        );
        fixture.record(&raw).await.expect("batch");
        relation
            .handle(with_call_id(
                "blackboard_relate",
                json!({"idempotencyKey": "standalone",
            "fromEntryId": stable_id("entry", PROJECT_ID, "finding-1"),
            "toEntryId": stable_id("entry", PROJECT_ID, "finding-2"),
            "kind": "supports", "confidenceBasisPoints": 8000}),
                &raw,
            ))
            .await
            .expect("standalone relation");
        let first_id =
            BlackboardEntryId::parse(stable_id("entry", PROJECT_ID, "finding-1")).unwrap();
        let relations = fixture
            .services
            .blackboard()
            .await
            .unwrap()
            .list_relations(PROJECT_ID, &first_id, /*max_results*/ 256)
            .await
            .unwrap();
        assert_eq!(
            relations
                .into_iter()
                .map(|relation| relation.value.provenance.source_id)
                .collect::<Vec<_>>(),
            vec![crate::call_source::call_source(&raw).unwrap(); 2]
        );
    }
}

#[tokio::test]
async fn invalid_raw_fallbacks_preserve_identity_conflicts() {
    for raw in [
        "x".repeat(/*n*/ 539),
        "x".repeat(/*n*/ 67_222),
        "call\ncontrol".to_string(),
        " padded ".to_string(),
    ] {
        let fixture = fixture().await;
        fixture.record(&raw).await.expect("batch");
        fixture.update(&raw).await.expect("obligation");
        let before = (fixture.findings().await, fixture.obligation_sources().await);
        let mut changed = progress();
        changed["packet"]["learning"] = json!(["different payload"]);
        let error = fixture
            .obligations
            .handle(with_call_id("obligation_update", changed, &raw))
            .await
            .err()
            .expect("identity conflict");
        assert!(
            error
                .to_string()
                .contains("obligation ID was already used for different content"),
            "{error}"
        );
        let relation = super::blackboard_write::BlackboardRelateTool::new(
            PROJECT_ID.to_string(),
            fixture.services.clone(),
            /*event_sink*/ None,
        );
        let error = relation
            .handle(with_call_id(
                "blackboard_relate",
                json!({
                    "idempotencyKey": "relation-1",
                    "fromEntryId": stable_id("entry", PROJECT_ID, "finding-1"),
                    "toEntryId": stable_id("entry", PROJECT_ID, "finding-2"),
                    "kind": "supports", "confidenceBasisPoints": 8000, "note": "different payload"
                }),
                &raw,
            ))
            .await
            .err()
            .expect("identity conflict");
        assert!(
            error
                .to_string()
                .contains("blackboard relation ID was already used for different content"),
            "{error}"
        );
        assert_eq!(
            (fixture.findings().await, fixture.obligation_sources().await),
            before
        );
    }
}

#[tokio::test]
async fn durable_completion_stores_digest_sources_for_large_call_ids() {
    use codex_stateful_runtime::AcceptanceChange;
    use codex_stateful_runtime::AcceptanceKind;
    use codex_stateful_runtime::AcceptanceOrigin;
    use codex_stateful_runtime::CriterionTerms;
    use codex_stateful_runtime::RequestSpan;
    use codex_stateful_runtime::StatefulRunStatus;
    use codex_thread_store::LocalThreadStore;
    use codex_thread_store::LocalThreadStoreConfig;

    for size in [539, 67_222] {
        let home = TempDir::new().expect("state home");
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let state = codex_state::StateRuntime::init(sqlite.clone(), "test".to_string())
            .await
            .expect("state DB");
        std::fs::write(home.path().join("verify.sh"), "checker").unwrap();
        std::fs::write(home.path().join("out.txt"), "verified output").unwrap();
        let project = state
            .create_project(
                "Calculator".to_string(),
                vec![codex_state::ProjectRoot {
                    path: home.path().to_string_lossy().into_owned(),
                }],
                Default::default(),
                &[],
                "project",
            )
            .await
            .expect("project")
            .project;
        let projects = Arc::new(LocalThreadStore::new(
            LocalThreadStoreConfig {
                codex_home: home.path().to_path_buf(),
                sqlite: sqlite.clone(),
                default_model_provider_id: "test".to_string(),
            },
            Some(state),
        ));
        let services = ProjectIntelligenceServices::new(sqlite);
        let runtime = services.runtime().await.expect("runtime");
        let goal = "Investigate calc.add.";
        let run = runtime
            .create_run(
                StatefulRunId::parse("completion-run").unwrap(),
                NewStatefulRun {
                    project_id: project.id.clone(),
                    thread_ids: vec![THREAD_ID.to_string()],
                    goal: goal.to_string(),
                    mode: WorkflowMode::Collaborative,
                    budget: RunBudget {
                        max_continuations: 1,
                        max_elapsed_seconds: 3600,
                    },
                },
            )
            .await
            .expect("run");
        // Stage the admitted plan and passing receipt as in the runtime completion fixture;
        // completion re-reads the pinned files and runs the normal acceptance gate.
        runtime
            .revise_acceptance(
                &run.id,
                /*expected_revision*/ 0,
                vec![AcceptanceChange::Add {
                    origin: AcceptanceOrigin::User,
                    kind: AcceptanceKind::Check,
                    statement: goal.to_string(),
                    request_span: Some(RequestSpan {
                        start: 0,
                        end: goal.len(),
                    }),
                    terms: CriterionTerms {
                        required: true,
                        artifacts: vec!["out.txt".to_string()],
                        checker: vec!["verify.sh".to_string()],
                        check_command: Some("sh verify.sh".to_string()),
                        expected_observation: Some("verified output".to_string()),
                        ..Default::default()
                    },
                }],
                "acceptance-call",
                /*project_root*/ None,
            )
            .await
            .expect("acceptance staged");
        let ledger = runtime.acceptance_ledger(&run.id).await.unwrap();
        let roots = vec![home.path().to_string_lossy().into_owned()];
        let (artifacts, checkers) =
            crate::acceptance_observation::ledger_file_states(&roots, &ledger).await;
        let digest =
            |states: &std::collections::BTreeMap<u32, codex_stateful_runtime::ArtifactState>| {
                match &states[&1] {
                    codex_stateful_runtime::ArtifactState::Observed { digest, missing }
                        if missing.is_empty() =>
                    {
                        digest.clone()
                    }
                    state => panic!("fixture files unavailable: {state:?}"),
                }
            };
        let artifact_digest = digest(&artifacts);
        let checker_digest = digest(&checkers);
        let ledger = runtime
            .revise_acceptance(
                &run.id,
                ledger.revision,
                vec![AcceptanceChange::Admit {
                    ordinal: 1,
                    checker_digest: Some(checker_digest.clone()),
                }],
                "admit-call",
                Some(home.path().canonicalize().unwrap()),
            )
            .await
            .expect("admitted plan");
        runtime
            .record_command_evidence(
                &run.id,
                vec![codex_stateful_runtime::CommandEvidence {
                    ordinal: 1,
                    criterion_revision: ledger.criteria[0].revision,
                    outcome: codex_stateful_runtime::EvidenceOutcome::Passed,
                    command: "sh verify.sh".to_string(),
                    exit_code: Some(0),
                    output_tail: "ok".to_string(),
                    output_digest: "sha256:output".to_string(),
                    artifact_digest: Some(artifact_digest),
                    checker_digest: Some(checker_digest),
                    detail: None,
                    start_generation: ledger.workspace_generation,
                    source_id: "checker-call".to_string(),
                }],
            )
            .await
            .expect("passing receipt");
        let records = BlackboardBatchRecordTool::new(
            project.id.clone(),
            THREAD_ID.to_string(),
            services.clone(),
            projects.clone(),
            /*event_sink*/ None,
            VisibleRootRegistry::default(),
        );
        let mut root_finding = finding("final");
        root_finding["rootPromotion"] = json!("promoted");
        records
            .handle(with_call_id(
                "blackboard_record_batch",
                json!({"records": [root_finding]}),
                "finding-call",
            ))
            .await
            .expect("durable learning recorded");
        let (root_revision, aliases) =
            crate::completion::completion_root(&services, &project.id, THREAD_ID)
                .await
                .expect("completion root");
        assert_eq!(aliases, 1);
        let tool = super::run::StatefulRunUpdateTool::new(
            project.id.clone(),
            THREAD_ID.to_string(),
            services.clone(),
            projects,
            /*event_sink*/ None,
            VisibleRootRegistry::default(),
            crate::deliverable::TurnRunCommits::default(),
        );
        let raw = "x".repeat(size);
        let packet = codex_stateful_runtime::ObligationPacket {
            learning: vec!["calc.add finding final.".to_string()],
            ..Default::default()
        };
        let mut completion_call = with_call_id(
            "stateful_run_update",
            json!({
                "expectedRevision": run.revision, "status": "completed",
                "completionDisposition": "durableLearning", "openIssues": [],
                "rootRevision": root_revision, "materialRootFindings": ["E1"],
                "completionIdempotencyKey": "completion", "finalObligation": packet,
                "result": "calc.add finding final."
            }),
            &raw,
        );
        let sandbox = codex_file_system::FileSystemSandboxContext::from_legacy_sandbox_policy(
            codex_protocol::protocol::SandboxPolicy::DangerFullAccess,
            home.path().abs().into(),
        )
        .unwrap();
        let file_system = codex_exec_server::FileSystemEnvironmentAccessor::new(
            &codex_exec_server::LOCAL_FS,
            sandbox.clone(),
        );
        completion_call.environments = vec![codex_extension_api::ToolEnvironment::new(
            "local".to_string(),
            sandbox.cwd.clone(),
            &file_system,
        )];
        let output = tool
            .handle(completion_call)
            .await
            .expect("durable completion through extension");
        let output: Value = serde_json::from_str(&output.log_output()).unwrap();
        assert_eq!(output["status"], json!("completed"));
        let completed = runtime.get_run(&run.id).await.unwrap().unwrap();
        let obligation = runtime.latest_obligation(&run.id).await.unwrap().unwrap();
        let source = crate::call_source::call_source(&raw).unwrap();
        assert_eq!(source.len(), 88);
        assert!(source.starts_with("stateful:call:v1:sha256:"));
        assert_eq!(
            (completed.status, obligation.value),
            (
                StatefulRunStatus::Completed,
                codex_stateful_runtime::NewObligation {
                    project_id: project.id,
                    run_id: run.id,
                    packet,
                    provenance_source_id: source,
                },
            )
        );
    }
}
