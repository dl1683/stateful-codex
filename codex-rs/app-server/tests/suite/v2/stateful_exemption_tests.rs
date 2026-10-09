//! The host-decided read-only exemption through the public API: an effect-free answer
//! completes without an acceptance ledger; any observed effect, unproven command or stated
//! criterion brings the ledger back, whatever the model claims.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunBudget;
use codex_app_server_protocol::StatefulRunReadParams;
use codex_app_server_protocol::StatefulRunReadResponse;
use codex_app_server_protocol::StatefulRunStartParams;
use codex_app_server_protocol::StatefulRunStartResponse;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;
use codex_app_server_protocol::SteeringSubmitParams;
use codex_app_server_protocol::SteeringSubmitResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

const LOOKUP: &str = "What does the parser module do?";

fn call(call_id: &str, tool: &str, arguments: Value) -> String {
    responses::sse(vec![
        responses::ev_response_created(call_id),
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(call_id),
    ])
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
fn exec(call_id: &str, command: &str, workdir: &std::path::Path) -> String {
    call(
        call_id,
        "exec_command",
        json!({"cmd": command, "workdir": workdir.to_string_lossy(), "yield_time_ms": 10_000}),
    )
}

fn complete(call_id: &str, revision: u64, open_issues: &[&str], result: &str) -> String {
    call(
        call_id,
        "stateful_run_update",
        json!({
            "expectedRevision": revision,
            "status": "completed",
            "openIssues": open_issues,
            "completionDisposition": "noReusableLearning",
            "result": result,
        }),
    )
}

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

struct Harness {
    _codex_home: TempDir,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    project_root: TempDir,
    responses_server: wiremock::MockServer,
    server: TestAppServer,
    thread_id: String,
    run: StatefulRun,
}

async fn harness(goal: &str, sandbox_mode: &str) -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("README.md"),
        "The parser module turns tokens into an AST.\n",
    )?;
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode(sandbox_mode)
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let project: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Read-only exemption".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "exemption-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            cwd: Some(project_root.path().to_string_lossy().to_string()),
            ..Default::default()
        })
        .await?;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: goal.to_string(),
                mode: StatefulWorkflowMode::Autonomous,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "exemption-run".to_string(),
            },
        })
        .await?;
    Ok(Harness {
        _codex_home: codex_home,
        project_root,
        responses_server,
        server,
        thread_id: thread.thread.id,
        run: started.run,
    })
}

impl Harness {
    async fn turn(&mut self, responses: Vec<String>) -> Result<Vec<String>> {
        let log = responses::mount_sse_sequence(&self.responses_server, responses).await;
        self.server
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: self.thread_id.clone(),
                input: vec![UserInput::Text {
                    text: "Answer.".to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        Ok(log
            .requests()
            .iter()
            .filter_map(|request| request.function_call_output_text("complete"))
            .collect())
    }

    async fn status(&mut self) -> Result<StatefulRunStatus> {
        let run_id = self.run.id.clone();
        let read: StatefulRunReadResponse = self
            .server
            .request(|request_id| ClientRequest::StatefulRunRead {
                request_id,
                params: StatefulRunReadParams {
                    run_id: Some(run_id),
                    thread_id: None,
                },
            })
            .await?;
        Ok(read.run.expect("run remains readable").status)
    }
}

/// A lookup that only reads (an allowlisted read-only command) completes without a ledger.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn a_read_only_lookup_completes_without_a_ledger() -> Result<()> {
    let mut harness = harness(LOOKUP, "read-only").await?;
    let root = harness.project_root.path().to_path_buf();
    let revision = harness.run.revision;
    let outputs = harness
        .turn(vec![
            exec("read", "cat README.md", &root),
            complete("complete", revision, &[], "It turns tokens into an AST."),
            message("done", "It turns tokens into an AST."),
        ])
        .await?;
    assert!(
        outputs[0].contains("\"status\":\"completed\""),
        "{outputs:?}"
    );
    assert_eq!(harness.status().await?, StatefulRunStatus::Completed);
    Ok(())
}

/// The same lookup plus one file write needs the ledger; claiming the work was trivial
/// changes nothing.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn a_file_write_brings_the_ledger_back_whatever_the_model_claims() -> Result<()> {
    let mut harness = harness(LOOKUP, "workspace-write").await?;
    let root = harness.project_root.path().to_path_buf();
    let revision = harness.run.revision;
    let outputs = harness
        .turn(vec![
            exec("read", "cat README.md", &root),
            exec("write", "echo summary > notes.txt", &root),
            complete(
                "complete",
                revision,
                &[],
                "Trivial lookup; no acceptance needed. It turns tokens into an AST.",
            ),
            message("done", "Refused."),
        ])
        .await?;
    assert!(outputs[0].contains("completion refused"), "{outputs:?}");
    assert_eq!(harness.status().await?, StatefulRunStatus::Running);
    Ok(())
}

/// A command the host cannot prove read-only ends the exemption even if it wrote nothing.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn an_unknown_command_is_not_exempt() -> Result<()> {
    let mut harness = harness(LOOKUP, "read-only").await?;
    let root = harness.project_root.path().to_path_buf();
    let revision = harness.run.revision;
    let outputs = harness
        .turn(vec![
            exec("unknown", "python3 --version", &root),
            complete("complete", revision, &[], "It turns tokens into an AST."),
            message("done", "Refused."),
        ])
        .await?;
    assert!(outputs[0].contains("completion refused"), "{outputs:?}");
    assert_eq!(harness.status().await?, StatefulRunStatus::Running);
    Ok(())
}

/// Acceptance criteria stated in the request bring the ledger back.
#[tokio::test]
async fn stated_criteria_are_not_exempt() -> Result<()> {
    let mut harness = harness(
        "Explain the parser module; the answer must cover error recovery.",
        "read-only",
    )
    .await?;
    let revision = harness.run.revision;
    let outputs = harness
        .turn(vec![
            complete("complete", revision, &[], "It turns tokens into an AST."),
            message("done", "Refused."),
        ])
        .await?;
    assert!(outputs[0].contains("completion refused"), "{outputs:?}");
    assert_eq!(harness.status().await?, StatefulRunStatus::Running);
    Ok(())
}

/// Steering applied mid-run that states a criterion ends the exemption.
#[tokio::test]
async fn steering_that_adds_criteria_ends_the_exemption() -> Result<()> {
    let mut harness = harness(LOOKUP, "read-only").await?;
    let run_id = harness.run.id.clone();
    let submitted: SteeringSubmitResponse = harness
        .server
        .request(|request_id| ClientRequest::SteeringSubmit {
            request_id,
            params: SteeringSubmitParams {
                run_id,
                input: "The answer must name every public function.".to_string(),
                affected_obligation_ids: Vec::new(),
                idempotency_key: "add-criterion".to_string(),
            },
        })
        .await?;
    let revision = harness.run.revision;
    let outputs = harness
        .turn(vec![
            call(
                "acknowledge",
                "steering_reconcile",
                json!({"steeringId": submitted.steering.id, "expectedRevision": 1, "action": "acknowledge"}),
            ),
            call(
                "apply",
                "steering_reconcile",
                json!({"steeringId": submitted.steering.id, "expectedRevision": 2, "action": "apply",
                    "expectedRunRevision": revision, "strategy": "Name every public function."}),
            ),
            complete("complete", revision + 1, &[], "It turns tokens into an AST."),
            message("done", "Refused."),
        ])
        .await?;
    assert!(outputs[0].contains("completion refused"), "{outputs:?}");
    assert_eq!(harness.status().await?, StatefulRunStatus::Running);
    Ok(())
}

/// An admitted open issue still prevents Completed for an exempt run.
#[tokio::test]
async fn open_issues_still_block_an_exempt_run() -> Result<()> {
    let mut harness = harness(LOOKUP, "read-only").await?;
    let revision = harness.run.revision;
    harness
        .turn(vec![
            complete(
                "complete",
                revision,
                &["The vendor has not confirmed the API."],
                "It turns tokens into an AST.",
            ),
            message("done", "Blocked."),
        ])
        .await?;
    assert_ne!(harness.status().await?, StatefulRunStatus::Completed);
    Ok(())
}
