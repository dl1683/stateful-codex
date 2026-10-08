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

const GOAL: &str = "Write out.txt.";

fn call(call_id: &str, tool: &str, arguments: Value) -> String {
    responses::sse(vec![
        responses::ev_response_created(call_id),
        responses::ev_function_call(call_id, tool, &arguments.to_string()),
        responses::ev_completed(call_id),
    ])
}

#[cfg(not(target_os = "windows"))]
fn exec(call_id: &str, command: &str, workdir: &std::path::Path) -> String {
    call(
        call_id,
        "exec_command",
        json!({"cmd": command, "workdir": workdir.to_string_lossy(), "yield_time_ms": 10_000}),
    )
}

#[cfg(not(target_os = "windows"))]
fn complete(call_id: &str, revision: u64) -> String {
    call(
        call_id,
        "stateful_run_update",
        json!({
            "expectedRevision": revision,
            "status": "completed",
            "openIssues": [],
            "completionDisposition": "noReusableLearning",
            "result": "out.txt is written.",
        }),
    )
}

fn message(id: &str, text: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message(id, text),
        responses::ev_completed(id),
    ])
}

fn declare(command: &str) -> String {
    call(
        "declare",
        "stateful_acceptance_update",
        json!({"expectedLedgerRevision": 0, "changes": [
            {"action": "add", "origin": "user", "kind": "deliverable", "statement": "out.txt is written.", "requestQuote": GOAL, "checkCommand": command, "expectedObservation": "verify.sh exits 0 once out.txt exists", "artifacts": ["out.txt"], "checker": ["verify.sh"]}
        ]}),
    )
}

fn admit(call_id: &str, revision: u64) -> String {
    call(
        call_id,
        "stateful_acceptance_update",
        json!({"expectedLedgerRevision": revision, "changes": [{"action": "admit", "criterion": "C1"}]}),
    )
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

async fn harness(mode: StatefulWorkflowMode) -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(project_root.path().join("verify.sh"), "test -f out.txt\n")?;
    // The run writes its outputs; a read-only sandbox would deny them.
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode("danger-full-access")
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
                name: "Acceptance gate".to_string(),
                roots: vec![ProjectRoot {
                    path: AbsolutePathBuf::try_from(project_root.path().to_path_buf())
                        .expect("temporary project root is absolute"),
                }],
                metadata: None,
                idempotency_key: "acceptance-project".to_string(),
            },
        })
        .await?;
    let thread = server
        .start_thread(ThreadStartParams {
            project_id: Some(project.project.id.clone()),
            ..Default::default()
        })
        .await?;
    let started: StatefulRunStartResponse = server
        .request(|request_id| ClientRequest::StatefulRunStart {
            request_id,
            params: StatefulRunStartParams {
                project_id: project.project.id,
                thread_id: thread.thread.id.clone(),
                goal: GOAL.to_string(),
                mode,
                budget: StatefulRunBudget {
                    max_continuations: 1,
                    max_elapsed_seconds: 3_600,
                },
                idempotency_key: "acceptance-run".to_string(),
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
    async fn turn(&mut self, text: &str) -> Result<()> {
        self.server
            .start_turn_and_wait_for_completion(TurnStartParams {
                thread_id: self.thread_id.clone(),
                input: vec![UserInput::Text {
                    text: text.to_string(),
                    text_elements: Vec::new(),
                }],
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    async fn read_run(&mut self) -> Result<StatefulRun> {
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
        Ok(read.run.expect("run remains readable"))
    }
}

/// Through the public API, unattended and without any steering: the host admits the check
/// plan, a failing check refuses completion, the repair and a passing check of the frozen
/// checker complete the run; editing the checker after admission invalidates it until the
/// host admits the new bytes.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn autonomous_run_completes_unattended_on_its_admitted_plan() -> Result<()> {
    let mut harness = harness(StatefulWorkflowMode::Autonomous).await?;
    let root = harness.project_root.path().to_path_buf();
    let revision = harness.run.revision;
    let response_log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            declare("sh verify.sh"),
            admit("admit", 1),
            exec("check-fails", "sh verify.sh", &root),
            complete("premature", revision),
            exec("repair", "echo done > out.txt", &root),
            exec("weaken", "echo 'exit 0' > verify.sh", &root),
            exec("check-weakened", "sh verify.sh", &root),
            complete("weakened", revision),
            admit("readmit", 4),
            exec("check-passes", "sh verify.sh", &root),
            complete("verified", revision),
            message("done", "out.txt is written."),
        ],
    )
    .await;
    harness.turn("Write out.txt.").await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 12);
    let admitted = requests[2].function_call_output("admit").to_string();
    assert!(admitted.contains("plan admitted"), "{admitted}");
    let premature = requests[4].function_call_output("premature").to_string();
    assert!(premature.contains("`sh verify.sh` failed"), "{premature}");
    let weakened = requests[8].function_call_output("weakened").to_string();
    assert!(
        weakened.contains("the checker changed after its plan was admitted"),
        "{weakened}"
    );
    let verified = requests[11].function_call_output("verified").to_string();
    assert!(verified.contains("acceptanceBasis"), "{verified}");
    let run = harness.read_run().await?;
    assert_eq!(run.status, StatefulRunStatus::Completed);
    assert!(
        run.result
            .as_deref()
            .is_some_and(|result| result.contains("ran its host-admitted plan")),
        "{:?}",
        run.result
    );
    Ok(())
}

/// A command that runs no checker file is refused as a plan, so its passing receipt can never
/// settle the requirement (Collaborative run, all platforms).
#[tokio::test]
async fn a_superficial_check_cannot_be_admitted() -> Result<()> {
    let mut harness = harness(StatefulWorkflowMode::Collaborative).await?;
    let response_log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            declare("echo suite-ok"),
            admit("admit-echo", 1),
            message("refused", "The echo cannot verify out.txt."),
        ],
    )
    .await;
    harness.turn("Write out.txt.").await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 3);
    let refusal = requests[2].function_call_output("admit-echo").to_string();
    assert!(
        refusal.contains("names none of C1's checker files"),
        "{refusal}"
    );
    assert_eq!(harness.read_run().await?.status, StatefulRunStatus::Running);
    Ok(())
}
