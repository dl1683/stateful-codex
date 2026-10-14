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
    declare_for(GOAL, command, "out.txt")
}

fn declare_for(quote: &str, command: &str, artifact: &str) -> String {
    call(
        "declare",
        "stateful_acceptance_update",
        json!({"expectedLedgerRevision": 0, "changes": [
            {"action": "add", "origin": "user", "kind": "deliverable", "statement": quote, "requestQuote": quote, "checkCommand": command, "expectedObservation": "verify.sh exits 0 once the output is correct", "artifacts": [artifact], "checker": ["verify.sh"]}
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

async fn harness(mode: StatefulWorkflowMode, sandbox_mode: &str) -> Result<Harness> {
    harness_for(
        mode,
        sandbox_mode,
        GOAL,
        &[("verify.sh", "test -f out.txt\n")],
    )
    .await
}

async fn harness_for(
    mode: StatefulWorkflowMode,
    sandbox_mode: &str,
    goal: &str,
    files: &[(&str, &str)],
) -> Result<Harness> {
    let responses_server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    for (name, content) in files {
        std::fs::write(project_root.path().join(name), content)?;
    }
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

/// Through the public API, unattended, without any steering or approval, under the
/// workspace-write sandbox: a write the sandbox denies leaves no command item and is closed as
/// terminated with unknown effects, a failing check refuses completion, the repair and a
/// passing check of the frozen checker complete the run; editing the checker after admission
/// invalidates it until the host admits the new bytes.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn autonomous_run_repairs_and_completes_unattended_in_the_sandbox() -> Result<()> {
    let mut harness = harness(StatefulWorkflowMode::Autonomous, "workspace-write").await?;
    let root = harness.project_root.path().to_path_buf();
    let revision = harness.run.revision;
    let response_log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            declare("sh verify.sh"),
            admit("admit", 1),
            exec("denied", "touch /etc/stateful-acceptance-denied", &root),
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
    assert_eq!(requests.len(), 13);
    let admitted = requests[2].function_call_output("admit").to_string();
    assert!(admitted.contains("plan admitted"), "{admitted}");
    let denied = requests[4].function_call_output("denied").to_string();
    assert!(denied.contains("Read-only file system"), "{denied}");
    let premature = requests[5].function_call_output("premature").to_string();
    assert!(premature.contains("completion refused"), "{premature}");
    assert!(!premature.contains("not accounted for"), "{premature}");
    let weakened = requests[9].function_call_output("weakened").to_string();
    assert!(
        weakened.contains("the checker changed after its plan was admitted"),
        "{weakened}"
    );
    let verified = requests[12].function_call_output("verified").to_string();
    assert!(verified.contains("acceptanceBasis"), "{verified}");
    assert_eq!(std::fs::read_to_string(root.join("out.txt"))?, "done\n");
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

/// A command that only mentions the checker file does not execute it, so it is refused as a
/// plan and its passing receipt can never settle the requirement (Collaborative run).
#[tokio::test]
async fn a_checker_mention_cannot_be_admitted() -> Result<()> {
    let mut harness = harness(StatefulWorkflowMode::Collaborative, "read-only").await?;
    let response_log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            declare("echo verify.sh"),
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
        refusal.contains("does not execute one of C1's checker files"),
        "{refusal}"
    );
    assert_eq!(harness.read_run().await?.status, StatefulRunStatus::Running);
    Ok(())
}

/// NAMED LIMITATION: the host proves that the admitted checker ran unchanged against the
/// pinned artifacts, not that the checker is adequate. A superficial checker passes a wrong
/// answer, and the run completes with the check disclosed as agent-written.
#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn an_inadequate_admitted_checker_passes_a_wrong_answer() -> Result<()> {
    const SUM_GOAL: &str = "Write the sum of 2 and 3 to total.txt.";
    let mut harness = harness_for(
        StatefulWorkflowMode::Autonomous,
        "read-only",
        SUM_GOAL,
        &[("total.txt", "6\n"), ("verify.sh", "test -s total.txt\n")],
    )
    .await?;
    let root = harness.project_root.path().to_path_buf();
    let revision = harness.run.revision;
    let response_log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            declare_for(SUM_GOAL, "sh verify.sh", "total.txt"),
            admit("admit", 1),
            exec("check", "sh verify.sh", &root),
            complete("complete", revision),
            message("done", "total.txt is written."),
        ],
    )
    .await;
    harness.turn(SUM_GOAL).await?;
    assert_eq!(response_log.requests().len(), 5);
    let run = harness.read_run().await?;
    assert_eq!(run.status, StatefulRunStatus::Completed);
    assert!(
        run.result
            .as_deref()
            .is_some_and(|result| result.contains("agent-written check `sh verify.sh`")),
        "{:?}",
        run.result
    );
    Ok(())
}

/// Commits the harness's project root as an ordinary repository: several unpinned source
/// files, an ignored build directory, and the checker.
#[cfg(target_os = "linux")]
fn commit_repository(root: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::create_dir_all(root.join("docs"))?;
    std::fs::write(
        root.join("src/parser.py"),
        "def parse(text):\n    return text.split()\n",
    )?;
    std::fs::write(root.join("src/cli.py"), "from parser import parse\n")?;
    std::fs::write(root.join("docs/guide.md"), "# Guide\n")?;
    std::fs::write(root.join("README.md"), "# Parser\n")?;
    std::fs::write(root.join(".gitignore"), "build/\n")?;
    // Ordinary files whose names mimic repository-state keys cannot mask repository state.
    std::fs::write(root.join(".git#HEAD"), "ordinary\n")?;
    std::fs::write(root.join(".git#index"), "ordinary\n")?;
    for arguments in [
        &["init", "--quiet"][..],
        &["add", "."][..],
        &["commit", "--quiet", "-m", "initial"][..],
    ] {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(arguments)
            .current_dir(root)
            .status()?;
        anyhow::ensure!(status.success(), "git {arguments:?} failed");
    }
    Ok(())
}

/// In a real repository with many unpinned files, unattended Autonomous work repairs the
/// output, runs its admitted check and completes: unpinned files that the check leaves
/// unchanged do not stop it qualifying.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn autonomous_run_completes_in_a_repository_with_unpinned_files() -> Result<()> {
    let mut harness = harness(StatefulWorkflowMode::Autonomous, "workspace-write").await?;
    let root = harness.project_root.path().to_path_buf();
    commit_repository(&root)?;
    let revision = harness.run.revision;
    let response_log = responses::mount_sse_sequence(
        &harness.responses_server,
        vec![
            declare("sh verify.sh"),
            admit("admit", 1),
            exec("repair", "echo done > out.txt", &root),
            exec("check", "sh verify.sh", &root),
            complete("verified", revision),
            message("done", "out.txt is written."),
        ],
    )
    .await;
    harness.turn("Write out.txt.").await?;
    let requests = response_log.requests();
    assert_eq!(requests.len(), 6);
    let verified = requests[5].function_call_output("verified").to_string();
    assert!(verified.contains("acceptanceBasis"), "{verified}");
    assert_eq!(
        harness.read_run().await?.status,
        StatefulRunStatus::Completed
    );
    Ok(())
}

/// A check that rewrites a tracked unpinned file, creates an untracked one, commits, or
/// changes only the staged index is refused.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_check_that_changes_unpinned_repository_files_is_refused() -> Result<()> {
    for (checker, case) in [
        ("test -f out.txt && echo checked >> README.md\n", "tracked"),
        ("test -f out.txt && echo checked > check.log\n", "untracked"),
        (
            "test -f out.txt && git -c user.name=t -c user.email=t@example.com commit -q --allow-empty -m checked\n",
            "empty commit",
        ),
        (
            "test -f out.txt && git rm -q --cached docs/guide.md\n",
            "index only",
        ),
    ] {
        let mut harness = harness_for(
            StatefulWorkflowMode::Autonomous,
            // The checker writes the repository itself, which the workspace sandbox protects.
            "danger-full-access",
            GOAL,
            &[("verify.sh", checker)],
        )
        .await?;
        let root = harness.project_root.path().to_path_buf();
        commit_repository(&root)?;
        let revision = harness.run.revision;
        let response_log = responses::mount_sse_sequence(
            &harness.responses_server,
            vec![
                declare("sh verify.sh"),
                admit("admit", 1),
                exec("repair", "echo done > out.txt", &root),
                exec("check", "sh verify.sh", &root),
                complete("refused", revision),
                message("done", "The check changed the repository."),
            ],
        )
        .await;
        harness.turn("Write out.txt.").await?;
        let refused = response_log.requests()[5]
            .function_call_output("refused")
            .to_string();
        assert!(refused.contains("completion refused"), "{case}: {refused}");
        assert_eq!(
            harness.read_run().await?.status,
            StatefulRunStatus::Running,
            "{case}"
        );
    }
    Ok(())
}
