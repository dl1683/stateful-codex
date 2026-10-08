use std::collections::HashMap;

use codex_protocol::items::CommandExecutionItem;
use codex_protocol::items::FileChangeItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::PatchApplyStatus;
use codex_state::SqliteConfig;
use codex_stateful_runtime::AcceptanceChange;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::CriterionVerdict;
use codex_stateful_runtime::EvidenceOutcome;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::WorkflowMode;
use codex_stateful_runtime::criterion_verdict;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::artifact_states;
use super::observe_item;
use super::runs_check;
use crate::services::ProjectIntelligenceServices;

fn command_item(id: &str, script: &str, status: &str, exit_code: i32, parsed: &str) -> TurnItem {
    let item: CommandExecutionItem = serde_json::from_value(json!({
        "id": id,
        "command": ["/bin/bash", "-lc", script],
        "cwd": "file:///project",
        "parsed_cmd": [{"type": parsed, "cmd": script, "name": "f", "path": "f"}],
        "source": "agent",
        "status": status,
        "aggregated_output": format!("output of {script}"),
        "exit_code": exit_code,
    }))
    .expect("command item decodes");
    TurnItem::CommandExecution(item)
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
async fn artifact_digests_track_content_and_report_missing_or_outside_files() {
    let root = TempDir::new().expect("root");
    let outside = TempDir::new().expect("outside");
    std::fs::create_dir_all(root.path().join("out")).expect("out dir");
    std::fs::write(root.path().join("out/report.json"), "{}").expect("artifact written");
    std::fs::write(outside.path().join("secret.txt"), "x").expect("outside written");
    let roots = vec![root.path().to_string_lossy().to_string()];
    let criterion = |artifacts: Vec<String>| codex_stateful_runtime::AcceptanceCriterion {
        id: "run#C1".to_string(),
        ordinal: 1,
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Deliverable,
        state: codex_stateful_runtime::AcceptanceState::Active,
        statement: "Report exists.".to_string(),
        requirement: "Report exists.".to_string(),
        required: true,
        depends_on: Vec::new(),
        milestone: None,
        request_span: None,
        artifacts,
        check_command: None,
        expected_observation: None,
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
}

#[tokio::test]
async fn observed_checks_bind_evidence_and_later_mutations_make_it_stale() {
    let state = TempDir::new().expect("state");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state.path().abs()));
    let runtime = services.runtime().await.expect("runtime opens");
    let run_id = StatefulRunId::parse("observed-run").expect("run id");
    runtime
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
    runtime
        .revise_acceptance(
            &run_id,
            0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::User,
                kind: AcceptanceKind::Check,
                statement: "All tests must pass.".to_string(),
                request_span: Some(codex_stateful_runtime::RequestSpan { start: 0, end: 20 }),
                terms: codex_stateful_runtime::CriterionTerms {
                    required: true,
                    check_command: Some("pytest -q".to_string()),
                    expected_observation: Some("every test passes".to_string()),
                    ..Default::default()
                },
            }],
            "call-add",
        )
        .await
        .expect("criterion added");
    let verdict = || async {
        let ledger = runtime
            .acceptance_ledger(&run_id)
            .await
            .expect("ledger reads");
        let criterion = ledger.criterion(1).expect("criterion").clone();
        (
            criterion.evidence.as_ref().map(|evidence| evidence.outcome),
            criterion_verdict(&criterion, ledger.workspace_generation, None),
        )
    };

    // A read-only command neither binds nor invalidates; a declined check of required work
    // is recorded as unavailable and keeps the criterion unmet.
    observe_item(
        &services,
        &run_id,
        &[],
        &command_item("read", "cat setup.py", "completed", 0, "read"),
    )
    .await;
    observe_item(
        &services,
        &run_id,
        &[],
        &command_item("declined", "pytest -q", "declined", -1, "unknown"),
    )
    .await;
    assert_eq!(
        verdict().await,
        (
            Some(EvidenceOutcome::Unavailable),
            CriterionVerdict::Unmet(
                "required but unverified (the session's approval policy declined the check, so it did not run); required work cannot complete unverified. Repair it or find a safe check; if it cannot be verified, set the run blocked with a partial result that names it".to_string()
            )
        )
    );
    observe_item(
        &services,
        &run_id,
        &[],
        &command_item("green", "pytest -q", "completed", 0, "unknown"),
    )
    .await;
    assert_eq!(
        verdict().await,
        (
            Some(EvidenceOutcome::Passed),
            CriterionVerdict::SatisfiedByHost
        )
    );
    assert_eq!(
        runtime
            .acceptance_ledger(&run_id)
            .await
            .expect("ledger")
            .workspace_generation,
        0
    );

    // A later mutating command or applied patch makes the green result stale.
    observe_item(
        &services,
        &run_id,
        &[],
        &command_item("edit", "sed -i s/a/b/ f", "completed", 0, "unknown"),
    )
    .await;
    assert_eq!(
        verdict().await,
        (
            Some(EvidenceOutcome::Passed),
            CriterionVerdict::Unmet(
                "stale: the workspace changed after `pytest -q` passed; run it again".to_string()
            )
        )
    );
    observe_item(
        &services,
        &run_id,
        &[],
        &command_item("timed-out", "pytest -q", "failed", 124, "unknown"),
    )
    .await;
    // A verifier the shell tool timed out is a failed check, never a pass.
    assert_eq!(
        verdict().await,
        (
            Some(EvidenceOutcome::Failed),
            CriterionVerdict::Unmet(
                "`pytest -q` failed (exit 124); repair the work and run it again".to_string()
            )
        )
    );
    observe_item(
        &services,
        &run_id,
        &[],
        &command_item("fixed", "pytest -q", "completed", 0, "unknown"),
    )
    .await;
    assert_eq!(verdict().await.1, CriterionVerdict::SatisfiedByHost);
    observe_item(
        &services,
        &run_id,
        &[],
        &TurnItem::FileChange(FileChangeItem {
            id: "patch".to_string(),
            changes: HashMap::new(),
            status: Some(PatchApplyStatus::Completed),
            auto_approved: None,
            stdout: None,
            stderr: None,
        }),
    )
    .await;
    assert!(verdict().await.1.is_unmet());
}
