use std::collections::HashMap;
use std::path::Path;

use codex_extension_api::ExtensionData;
use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::PatchApplyStatus;
use codex_state::SqliteConfig;
use codex_stateful_runtime::AcceptanceChange;
use codex_stateful_runtime::AcceptanceCriterion;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::AcceptanceState;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::CriterionTerms;
use codex_stateful_runtime::EvidenceOutcome;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::WorkflowMode;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::MAX_OUTSTANDING_READERS;
use super::OUTSTANDING_READERS;
use super::artifact_states;
use super::bind_turn;
use super::command_started;
use super::forget_command;
use super::hash_bounded;
use super::observe_item;
use super::runs_check;
use crate::services::ProjectIntelligenceServices;

fn file_uri(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

fn command_item(
    id: &str,
    script: &str,
    cwd: &Path,
    status: &str,
    exit_code: i32,
    parsed: &str,
) -> TurnItem {
    let item: CommandExecutionItem = serde_json::from_value(json!({
        "id": id,
        "command": ["/bin/bash", "-lc", script],
        "cwd": file_uri(cwd),
        "parsed_cmd": [{"type": parsed, "cmd": script, "name": "f", "path": "f"}],
        "source": "agent",
        "status": status,
        "aggregated_output": format!("output of {script}"),
        "exit_code": exit_code,
    }))
    .expect("command item decodes");
    TurnItem::CommandExecution(item)
}

fn argv(script: &str) -> Vec<String> {
    vec![
        "/bin/bash".to_string(),
        "-lc".to_string(),
        script.to_string(),
    ]
}

struct Fixture {
    _state: TempDir,
    project: TempDir,
    services: ProjectIntelligenceServices,
    store: StatefulRunStore,
    run_id: StatefulRunId,
    roots: Vec<String>,
    turn: ExtensionData,
}

async fn fixture(run: &str) -> Fixture {
    let state = TempDir::new().expect("state");
    let project = TempDir::new().expect("project");
    std::fs::create_dir_all(project.path().join("src")).expect("src");
    std::fs::write(project.path().join("src/parse.py"), "v1").expect("source");
    std::fs::create_dir_all(project.path().join("tests")).expect("tests");
    std::fs::write(
        project.path().join("tests/test_parse.py"),
        "def test(): pass",
    )
    .expect("checker");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state.path().abs()));
    let store = services.runtime().await.expect("runtime").clone();
    let run_id = StatefulRunId::parse(run).expect("run id");
    store
        .create_run(
            run_id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "All tests must pass.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 24,
                    max_elapsed_seconds: 14_400,
                },
            },
        )
        .await
        .expect("run created");
    store
        .revise_acceptance(
            &run_id,
            0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Check,
                statement: "Unit tests pass.".to_string(),
                request_span: None,
                terms: CriterionTerms {
                    required: true,
                    artifacts: vec!["src/parse.py".to_string()],
                    checker: vec!["tests/test_parse.py".to_string()],
                    check_command: Some("pytest -q tests/test_parse.py".to_string()),
                    expected_observation: Some("every test passes".to_string()),
                    ..CriterionTerms::default()
                },
            }],
            "call-add",
        )
        .await
        .expect("criterion added");
    let turn = ExtensionData::new("turn");
    bind_turn(&services, &turn, &run_id).await;
    Fixture {
        roots: vec![project.path().to_string_lossy().to_string()],
        _state: state,
        project,
        services,
        store,
        run_id,
        turn,
    }
}

impl Fixture {
    async fn start(&self, id: &str, script: &str, cwd: &Path) {
        command_started(
            &self.services,
            &self.roots,
            &self.turn,
            id,
            &argv(script),
            cwd,
        )
        .await;
    }

    async fn finish(
        &self,
        id: &str,
        script: &str,
        cwd: &Path,
        status: &str,
        exit_code: i32,
        parsed: &str,
    ) {
        observe_item(
            &self.services,
            &self.roots,
            &self.turn,
            &command_item(id, script, cwd, status, exit_code, parsed),
        )
        .await;
    }

    async fn check(&self, id: &str, status: &str, exit_code: i32) {
        let root = self.project.path().to_path_buf();
        self.start(id, "pytest -q tests/test_parse.py", &root).await;
        self.finish(
            id,
            "pytest -q tests/test_parse.py",
            &root,
            status,
            exit_code,
            "unknown",
        )
        .await;
    }

    async fn pending(&self) -> u64 {
        self.store
            .acceptance_ledger(&self.run_id)
            .await
            .expect("ledger")
            .pending_commands
    }

    async fn latest(&self) -> (Option<EvidenceOutcome>, u64) {
        let ledger = self
            .store
            .acceptance_ledger(&self.run_id)
            .await
            .expect("ledger");
        (
            ledger.criteria[0]
                .evidence
                .as_ref()
                .map(|evidence| evidence.outcome),
            ledger.workspace_generation,
        )
    }
}

#[test]
fn checks_match_the_executed_script_exactly() {
    let argv = |parts: &[&str]| parts.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert!(runs_check(
        &argv(&["/bin/bash", "-lc", "pytest -q"]),
        "pytest -q"
    ));
    assert!(runs_check(
        &argv(&["cmd.exe", "/d", "/c", "pytest -q"]),
        "pytest -q"
    ));
    assert!(runs_check(
        &argv(&["pwsh", "-NoProfile", "-Command", "pytest -q"]),
        "pytest -q"
    ));
    assert!(runs_check(&argv(&["pytest", "-q"]), "pytest -q"));
    assert!(!runs_check(
        &argv(&["/bin/bash", "-lc", "pytest -q || true"]),
        "pytest -q"
    ));
    assert!(!runs_check(
        &argv(&["/bin/bash", "-lc", "pytest"]),
        "pytest -q"
    ));
}

#[tokio::test]
async fn artifact_digests_track_content_and_refuse_unpinnable_files() {
    let root = TempDir::new().expect("root");
    let outside = TempDir::new().expect("outside");
    std::fs::create_dir_all(root.path().join("out")).expect("out dir");
    std::fs::write(root.path().join("out/report.json"), "{}").expect("artifact written");
    std::fs::write(outside.path().join("secret.txt"), "x").expect("outside written");
    let large = vec![0_u8; 16 * 1024 * 1024 + 1];
    std::fs::write(root.path().join("out/large.bin"), large).expect("large written");
    let roots = vec![root.path().to_string_lossy().to_string()];
    let criterion = |artifacts: Vec<String>| AcceptanceCriterion {
        id: "run#C1".to_string(),
        ordinal: 1,
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Existence,
        state: AcceptanceState::Active,
        statement: "Report exists.".to_string(),
        requirement: "Report exists.".to_string(),
        required: true,
        depends_on: Vec::new(),
        milestone: None,
        request_span: None,
        artifacts,
        checker: Vec::new(),
        check_command: None,
        check_cwd: None,
        expected_observation: None,
        plan: None,
        dismissal: None,
        note: None,
        revision: 1,
        ledger_revision: 1,
        evidence: None,
    };
    let present = criterion(vec!["out/report.json".to_string()]);
    let first = artifact_states(&roots, std::iter::once(&present)).await;
    std::fs::write(root.path().join("out/report.json"), "{\"a\":1}").expect("artifact changed");
    let second = artifact_states(&roots, std::iter::once(&present)).await;
    let (
        Some(ArtifactState::Observed {
            digest: before,
            missing,
        }),
        Some(ArtifactState::Observed { digest: after, .. }),
    ) = (first.get(&1), second.get(&1))
    else {
        panic!("artifacts are observed: {first:?} {second:?}");
    };
    assert_eq!(missing, &Vec::<String>::new());
    assert_ne!(before, after);
    let absent = criterion(vec!["out/missing.csv".to_string()]);
    assert!(matches!(
        artifact_states(&roots, std::iter::once(&absent)).await.get(&1),
        Some(ArtifactState::Observed { missing, .. }) if missing == &vec!["out/missing.csv".to_string()]
    ));
    let escaped = criterion(vec![
        outside
            .path()
            .join("secret.txt")
            .to_string_lossy()
            .to_string(),
    ]);
    assert!(matches!(
        artifact_states(&roots, std::iter::once(&escaped)).await.get(&1),
        Some(ArtifactState::Unavailable(reason)) if reason.contains("outside the project roots")
    ));
    // Size and modification time are never content identity.
    let oversized = criterion(vec!["out/large.bin".to_string()]);
    assert!(matches!(
        artifact_states(&roots, std::iter::once(&oversized)).await.get(&1),
        Some(ArtifactState::Unavailable(reason)) if reason.contains("cannot be pinned")
    ));
}

#[tokio::test]
async fn a_check_binds_only_in_its_directory_with_a_start_snapshot() {
    let fixture = fixture("run-directory").await;
    let elsewhere = TempDir::new().expect("other repository");
    let root = fixture.project.path().to_path_buf();
    // The same command text in an unrelated directory is no evidence (and is a mutation).
    fixture
        .start("other", "pytest -q tests/test_parse.py", elsewhere.path())
        .await;
    fixture
        .finish(
            "other",
            "pytest -q tests/test_parse.py",
            elsewhere.path(),
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(fixture.latest().await, (None, 1));
    // No start snapshot: unavailable, never passed.
    fixture
        .finish(
            "unseen",
            "pytest -q tests/test_parse.py",
            &root,
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(fixture.latest().await.0, Some(EvidenceOutcome::Unavailable));
    // A clean run in the pinned directory passes and tracks in-flight state.
    fixture
        .start("clean", "pytest -q tests/test_parse.py", &root)
        .await;
    assert_eq!(fixture.pending().await, 1);
    fixture
        .finish(
            "clean",
            "pytest -q tests/test_parse.py",
            &root,
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(fixture.pending().await, 0);
    assert_eq!(fixture.latest().await, (Some(EvidenceOutcome::Passed), 1));
    // A verifier the shell tool timed out is failed, never passed.
    fixture.check("timed-out", "failed", 124).await;
    assert_eq!(fixture.latest().await.0, Some(EvidenceOutcome::Failed));
}

#[tokio::test]
async fn changes_while_a_check_runs_make_it_unavailable() {
    let fixture = fixture("run-race").await;
    let root = fixture.project.path().to_path_buf();
    // The pinned input changes while the check runs.
    fixture
        .start("edit-during", "pytest -q tests/test_parse.py", &root)
        .await;
    std::fs::write(root.join("src/parse.py"), "v2").expect("edited");
    fixture
        .finish(
            "edit-during",
            "pytest -q tests/test_parse.py",
            &root,
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(fixture.latest().await.0, Some(EvidenceOutcome::Unavailable));
    // Another observed mutation lands between the check's start and end.
    fixture
        .start("concurrent", "pytest -q tests/test_parse.py", &root)
        .await;
    fixture.start("writer", "sed -i s/a/b/ f", &root).await;
    fixture
        .finish(
            "writer",
            "sed -i s/a/b/ f",
            &root,
            "completed",
            0,
            "unknown",
        )
        .await;
    fixture
        .finish(
            "concurrent",
            "pytest -q tests/test_parse.py",
            &root,
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(fixture.latest().await.0, Some(EvidenceOutcome::Unavailable));
}

#[tokio::test]
async fn a_late_result_stays_with_the_run_that_started_it() {
    let fixture = fixture("run-a").await;
    let root = fixture.project.path().to_path_buf();
    fixture
        .start("background", "pytest -q tests/test_parse.py", &root)
        .await;
    // Run B starts on the same thread with the same check, in a new turn.
    let run_b = StatefulRunId::parse("run-b").expect("run id");
    fixture
        .store
        .create_run(
            run_b.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "All tests must pass.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 24,
                    max_elapsed_seconds: 14_400,
                },
            },
        )
        .await
        .expect("run b");
    let turn_b = ExtensionData::new("turn-b");
    bind_turn(&fixture.services, &turn_b, &run_b).await;
    // A's delayed result arrives with A's originating turn store.
    fixture
        .finish(
            "background",
            "pytest -q tests/test_parse.py",
            &root,
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(
        fixture
            .store
            .acceptance_ledger(&run_b)
            .await
            .expect("ledger")
            .criteria,
        Vec::new()
    );
    assert_eq!(fixture.latest().await.0, Some(EvidenceOutcome::Passed));
}

#[tokio::test]
async fn patches_and_reads_affect_the_generation_conservatively() {
    let fixture = fixture("run-patches").await;
    let root = fixture.project.path().to_path_buf();
    fixture
        .finish("read", "cat setup.py", &root, "completed", 0, "read")
        .await;
    assert_eq!(fixture.latest().await.1, 0);
    let patch = |status: PatchApplyStatus| {
        TurnItem::FileChange(FileChangeItem {
            id: "patch".to_string(),
            changes: HashMap::new(),
            status: Some(status),
            auto_approved: None,
            stdout: None,
            stderr: None,
        })
    };
    observe_item(
        &fixture.services,
        &fixture.roots,
        &fixture.turn,
        &patch(PatchApplyStatus::Declined),
    )
    .await;
    assert_eq!(fixture.latest().await.1, 0);
    // A failed patch may have applied a prefix.
    observe_item(
        &fixture.services,
        &fixture.roots,
        &fixture.turn,
        &patch(PatchApplyStatus::Failed),
    )
    .await;
    assert_eq!(fixture.latest().await.1, 1);
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .expect("ledger");
    assert_eq!(ledger.observed_executions, 1);
}

#[tokio::test]
async fn a_declared_check_that_mutates_governing_files_invalidates_earlier_evidence() {
    let fixture = fixture("run-mutating-check").await;
    let root = fixture.project.path().to_path_buf();
    std::fs::write(root.join("gen.sh"), "generator").expect("generator");
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .expect("ledger");
    fixture
        .store
        .revise_acceptance(
            &fixture.run_id,
            ledger.revision,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Check,
                statement: "The generator runs.".to_string(),
                request_span: None,
                terms: CriterionTerms {
                    required: true,
                    artifacts: vec!["out.txt".to_string()],
                    checker: vec!["gen.sh".to_string()],
                    check_command: Some("sh gen.sh".to_string()),
                    expected_observation: Some("exit 0".to_string()),
                    ..CriterionTerms::default()
                },
            }],
            "call-c2",
        )
        .await
        .expect("C2");
    std::fs::write(root.join("out.txt"), "out").expect("output");
    fixture.check("c1-pass", "completed", 0).await;
    assert_eq!(fixture.latest().await, (Some(EvidenceOutcome::Passed), 0));
    // C2 is a declared check, but it rewrites C1's pinned input.
    fixture.start("c2", "sh gen.sh", &root).await;
    std::fs::write(root.join("src/parse.py"), "regenerated").expect("rewritten");
    fixture
        .finish("c2", "sh gen.sh", &root, "completed", 0, "unknown")
        .await;
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .expect("ledger");
    assert_eq!(ledger.workspace_generation, 1);
    let c1 = ledger
        .criterion(1)
        .expect("C1")
        .evidence
        .clone()
        .expect("evidence");
    assert_eq!(c1.workspace_generation, 0, "C1's earlier pass is now stale");
    let c2 = ledger
        .criterion(2)
        .expect("C2")
        .evidence
        .clone()
        .expect("evidence");
    assert_eq!(
        (c2.outcome, c2.workspace_generation),
        (EvidenceOutcome::Passed, 1)
    );
}

#[tokio::test]
async fn pending_commands_clear_only_after_accounting_or_a_proven_non_launch() {
    let fixture = fixture("run-pending").await;
    let root = fixture.project.path().to_path_buf();
    fixture.start("never-launched", "make", &root).await;
    fixture.start("aborted", "sleep 100", &root).await;
    assert_eq!(fixture.pending().await, 2);
    forget_command(&fixture.services, &fixture.turn, "never-launched").await;
    assert_eq!(fixture.pending().await, 1);
    // An aborted tool's process may still run: it stays pending until its end is accounted.
    fixture
        .finish("aborted", "sleep 100", &root, "completed", 0, "unknown")
        .await;
    assert_eq!(fixture.pending().await, 0);
}

#[test]
fn bounded_hashing_detects_growth_and_stops_at_the_deadline() {
    let never = || false;
    assert_eq!(
        hash_bounded(&b"abcd"[..], 4, &never),
        hash_bounded(&b"abcd"[..], 4, &never)
    );
    assert_eq!(
        hash_bounded(&b"abcdef"[..], 4, &never),
        Err("it grew while being read".to_string())
    );
    assert_eq!(
        hash_bounded(&b"ab"[..], 4, &never),
        Err("it changed size while being read".to_string())
    );
    assert_eq!(
        hash_bounded(&b"abcd"[..], 4, &|| true),
        Err("the read budget ran out".to_string())
    );
}

#[tokio::test]
async fn reader_capacity_is_bounded_and_recovers() {
    let fixture = fixture("run-capacity").await;
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .expect("ledger");
    let criterion = ledger.criterion(1).expect("C1").clone();
    OUTSTANDING_READERS.store(MAX_OUTSTANDING_READERS, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        artifact_states(&fixture.roots, std::iter::once(&criterion)).await.get(&1),
        Some(ArtifactState::Unavailable(reason)) if reason.contains("outstanding")
    ));
    OUTSTANDING_READERS.store(0, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        artifact_states(&fixture.roots, std::iter::once(&criterion))
            .await
            .get(&1),
        Some(ArtifactState::Observed { .. })
    ));
    assert_eq!(
        OUTSTANDING_READERS.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_fifo_artifact_is_refused_without_blocking_a_reader() {
    let fixture = fixture("run-fifo").await;
    let fifo = fixture.project.path().join("src/parse.py");
    std::fs::remove_file(&fifo).expect("remove");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo");
    assert!(status.success());
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .expect("ledger");
    let criterion = ledger.criterion(1).expect("C1").clone();
    let started = std::time::Instant::now();
    assert!(matches!(
        artifact_states(&fixture.roots, std::iter::once(&criterion)).await.get(&1),
        Some(ArtifactState::Unavailable(reason)) if reason.contains("not a regular file")
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(
        OUTSTANDING_READERS.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
