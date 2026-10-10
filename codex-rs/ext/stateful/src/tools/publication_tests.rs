use super::*;
use crate::memory_controls::MemoryActor;
use crate::memory_controls::forget_entry;
use crate::tools::capture_repair_tests::call;
use crate::visible_root::VisibleRootRegistry;
use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

const PROJECT_ID: &str = "project-1";

/// Answers every call with a fixed result, standing in for any Stateful tool.
struct Fixed {
    succeeds: bool,
}

impl<'call> ToolExecutor<ToolCall<'call>> for Fixed {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("fixed")
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: "fixed".to_string(),
            description: "Fixed result.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({"type": "object"})).expect("static schema"),
            output_schema: None,
        })
    }

    fn handle<'a>(&'a self, _call: ToolCall<'call>) -> ToolExecutorFuture<'a>
    where
        ToolCall<'call>: 'a,
    {
        let succeeds = self.succeeds;
        Box::pin(async move {
            if succeeds {
                Ok(
                    Box::new(JsonToolOutput::new(json!({"words": "Never push."})))
                        as Box<dyn codex_extension_api::ToolOutput>,
                )
            } else {
                Err(FunctionCallError::RespondToModel("refused".to_string()))
            }
        })
    }
}

struct Fixture {
    _home: TempDir,
    services: ProjectIntelligenceServices,
    /// A second pool on the same database: another process's writer.
    other: BlackboardStore,
}

impl Fixture {
    async fn new() -> Self {
        let home = TempDir::new().expect("home");
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let services = ProjectIntelligenceServices::new(sqlite.clone());
        let node_id = services.project_node_id(PROJECT_ID).await.expect("node");
        let store = services.blackboard().await.expect("store");
        for id in ["a", "b", "c"] {
            store
                .create_entry(
                    BlackboardEntryId::parse(id).expect("ID"),
                    NewBlackboardEntry {
                        project_id: PROJECT_ID.to_string(),
                        node_id: node_id.clone(),
                        kind: BlackboardKind::Decision,
                        content: format!("Decision {id}."),
                        structured_value: None,
                        confidence: ConfidenceScore::from_basis_points(9_000).expect("score"),
                        verification: BlackboardVerification::Unverified,
                        importance: BlackboardImportance::High,
                        root_promotion: RootPromotion::NotPromoted,
                        evidence: Vec::new(),
                        premises: Vec::new(),
                        provenance: BlackboardProvenance {
                            kind: BlackboardProvenanceKind::Agent,
                            source_id: "turn-1".to_string(),
                        },
                    },
                )
                .await
                .expect("entry");
        }
        let other = BlackboardStore::open(&sqlite).await.expect("second pool");
        Self {
            _home: home,
            services,
            other,
        }
    }

    fn fenced(&self, effect: Effect, succeeds: bool) -> Fenced {
        Fenced::new(
            Arc::new(Fixed { succeeds }),
            effect,
            PROJECT_ID.to_string(),
            self.services.clone(),
        )
    }

    async fn forget(&self, id: &str) {
        forget_entry(
            &self.other,
            &MemoryActor::default(),
            PROJECT_ID,
            &BlackboardEntryId::parse(id).expect("ID"),
            /*expected_revision*/ 1,
        )
        .await
        .expect("forget");
    }
}

/// Runs one call and takes its publication check, as the host does when the handler returns.
async fn finished(tool: &Fenced, call_id: &str) -> ToolPublicationCheck {
    let mut call = call("fixed", json!({}), 9_000, ToolCallSource::Direct);
    call.call_id = call_id.to_string();
    let _ = tool.handle(call).await;
    tool.publication_check(call_id)
        .expect("every Stateful result is checked")
}

#[tokio::test]
async fn a_held_result_is_published_only_under_the_writer_lock_and_while_current() {
    let fixture = Fixture::new().await;
    let tool = fixture.fenced(Effect::Read, /*succeeds*/ true);

    // Current: the guard holds the writer lock, so a Forget from another pool waits for the
    // recording and commits only after the guard is dropped.
    let check = finished(&tool, "current").await;
    let guard = (check.validate)()
        .await
        .expect("current result is published");
    let other = fixture.other.clone();
    let forget = tokio::spawn(async move {
        forget_entry(
            &other,
            &MemoryActor::default(),
            PROJECT_ID,
            &BlackboardEntryId::parse("a").expect("ID"),
            /*expected_revision*/ 1,
        )
        .await
        .map(|entry| entry.revision)
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!forget.is_finished(), "a Forget committed during recording");
    drop(guard);
    assert_eq!(forget.await.expect("join").expect("forget"), 2);

    // A Forget committed after the read and before recording withholds the result.
    let check = finished(&tool, "stale").await;
    fixture.forget("b").await;
    assert_eq!(check.withheld, STALE_READ);
    assert!((check.validate)().await.is_none());

    // A result the wrapper never saw (evicted, or unknown) is withheld.
    let unknown = tool.publication_check("unknown").expect("check");
    assert!((unknown.validate)().await.is_none());
}

#[tokio::test]
async fn a_busy_writer_lock_withholds_the_result_instead_of_waiting() {
    let fixture = Fixture::new().await;
    let tool = fixture.fenced(Effect::Read, /*succeeds*/ true);
    let check = finished(&tool, "busy").await;
    let held = fixture
        .other
        .acquire_completion_fence(Duration::from_secs(5))
        .await
        .expect("another writer holds the lock");
    let started = std::time::Instant::now();
    assert!((check.validate)().await.is_none());
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    drop(held);
    // The store stays writable for others once the lock is free.
    fixture.forget("c").await;
}

#[tokio::test]
async fn writers_say_whether_a_withheld_change_was_committed() {
    let fixture = Fixture::new().await;
    let committed = finished(&fixture.fenced(Effect::Write, /*succeeds*/ true), "w1").await;
    let failed = finished(&fixture.fenced(Effect::Write, /*succeeds*/ false), "w2").await;
    let read_error = finished(&fixture.fenced(Effect::Read, /*succeeds*/ false), "r1").await;
    fixture.forget("a").await;
    assert_eq!(
        (
            committed.withheld.as_str(),
            failed.withheld.as_str(),
            read_error.withheld.as_str()
        ),
        (STALE_COMMITTED_WRITE, STALE_FAILED_WRITE, STALE_READ)
    );
    for check in [committed, failed, read_error] {
        assert!((check.validate)().await.is_none());
    }
}

#[tokio::test]
async fn stateful_tools_are_never_exposed_to_nested_code_mode() {
    assert_eq!(
        [
            ToolExposure::Direct,
            ToolExposure::DirectModelOnly,
            ToolExposure::Deferred,
            ToolExposure::DeferredModelOnly,
            ToolExposure::CodeModeOnly,
            ToolExposure::Hidden,
        ]
        .map(direct_only),
        [
            ToolExposure::DirectModelOnly,
            ToolExposure::DirectModelOnly,
            ToolExposure::DeferredModelOnly,
            ToolExposure::DeferredModelOnly,
            ToolExposure::Hidden,
            ToolExposure::Hidden,
        ]
    );
    let Fixture {
        _home: _home_guard,
        services,
        ..
    } = Fixture::new().await;
    let tools = super::super::project_intelligence_tools(
        PROJECT_ID.to_string(),
        "00000000-0000-0000-0000-000000000001".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    let exposed = tools
        .iter()
        .filter(|tool| tool.exposure().is_available_in_code_mode())
        .map(|tool| tool.tool_name().to_string())
        .collect::<Vec<_>>();
    assert_eq!(exposed, Vec::<String>::new());
    assert!(
        tools
            .iter()
            .all(|tool| tool.publication_check("never-ran").is_some())
    );
}
