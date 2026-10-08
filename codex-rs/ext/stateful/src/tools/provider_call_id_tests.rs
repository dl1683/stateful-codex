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
use sha2::Digest;
use sha2::Sha256;
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

fn oversized_source(call_id: &str) -> String {
    format!(
        "stateful:oversized-call-id:v1:sha256:{:x}",
        Sha256::digest(call_id.as_bytes())
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
fn provenance_keeps_valid_ids_and_namespaces_only_oversized_ones() {
    let bridged = bridged_call_id();
    let source = |call_id: &str| {
        provenance_source_id(&with_call_id("obligation_update", json!({}), call_id))
            .map_err(|error| error.to_string())
    };
    let refused = |reason: &str| Err(format!("nothing was written: {reason}"));
    let no_id = "this tool call has no usable call ID (it is empty or contains control characters), so its provenance cannot be recorded";

    assert_eq!(
        [
            source("call_abc123"),
            source(&"c".repeat(/*n*/ 200)),
            source(&"c".repeat(/*n*/ 512)),
            source(" call_padded"),
            source(&bridged),
            source(&oversized_source(&bridged)),
            source(&format!(" {bridged}")),
            source(""),
            source("call\n1"),
        ],
        [
            Ok("call_abc123".to_string()),
            Ok("c".repeat(/*n*/ 200)),
            Ok("c".repeat(/*n*/ 512)),
            Ok(" call_padded".to_string()),
            Ok(oversized_source(&bridged)),
            refused(
                "this tool call's ID uses the reserved stateful:oversized-call-id: namespace, so its provenance would be ambiguous"
            ),
            refused(
                "this tool call's oversized ID has surrounding whitespace, so it does not identify one source"
            ),
            refused(no_id),
            refused(no_id),
        ]
    );
}

/// A clean ID between the old 128-byte cut and the stores' 512-byte limit was stored verbatim
/// before oversized-ID support; an exact retry must still match its stored values.
#[tokio::test]
async fn store_valid_ids_replay_records_relations_and_obligations_unchanged() {
    let fixture = fixture().await;
    let call_id = format!("call_{}", "7".repeat(/*n*/ 195));

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

#[tokio::test]
async fn bridged_call_ids_record_findings_and_obligations_under_the_digest_namespace() {
    let fixture = fixture().await;
    let bridged = bridged_call_id();

    let output = fixture
        .record(&bridged)
        .await
        .expect("the batch is accepted");
    fixture.update(&bridged).await.expect("obligation recorded");

    let source = oversized_source(&bridged);
    assert_eq!(
        (
            counts(&output),
            fixture.findings().await,
            fixture.obligation_sources().await
        ),
        (
            (json!(2), json!(0), json!(1), json!(0)),
            vec![Some((source.clone(), 1)), Some((source.clone(), 1))],
            vec![source],
        )
    );
}

#[tokio::test]
async fn missing_or_aliasing_call_ids_write_nothing() {
    let fixture = fixture().await;
    let alias = oversized_source(&bridged_call_id());
    let before = fixture
        .services
        .blackboard()
        .await
        .expect("blackboard opens")
        .project_revision(PROJECT_ID)
        .await
        .expect("revision reads");

    let mut refusals = Vec::new();
    for call_id in ["", "call\u{7}", alias.as_str()] {
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
        (vec![(true, true); 3], before, vec![None, None], Vec::new())
    );
}
