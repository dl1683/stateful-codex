use super::*;
use crate::acceptance_observation::checker_states;
use codex_stateful_runtime::TermsUpdate;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn w1_an_admitted_python_check_in_a_plain_directory_passes_without_detail() {
    let fixture = fixture("walk-python").await;
    let root = fixture.project.path();
    std::fs::write(root.join("check.py"), "print('OK')\n").unwrap();
    let ledger = fixture
        .store
        .revise_acceptance(
            &fixture.run_id,
            1,
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate {
                    checker: Some(vec!["check.py".to_string()]),
                    check_command: Some("python check.py".to_string()),
                    ..TermsUpdate::default()
                },
            }],
            "python-terms",
        )
        .await
        .unwrap();
    let states = checker_states(&fixture.roots, ledger.criteria.iter()).await;
    let ArtifactState::Observed { digest, .. } = states.get(&1).unwrap() else {
        panic!("checker must be readable")
    };
    fixture
        .store
        .revise_acceptance(
            &fixture.run_id,
            ledger.revision,
            vec![AcceptanceChange::Admit {
                ordinal: 1,
                checker_digest: Some(digest.clone()),
            }],
            "python-admit",
        )
        .await
        .unwrap();
    assert!(!root.join(".git").exists());
    fixture.start("python", "python check.py", root).await;
    let output = std::process::Command::new("python")
        .arg("check.py")
        .current_dir(root)
        .output()
        .expect("python preflight");
    assert!(output.status.success());
    fixture
        .finish("python", "python check.py", root, "completed", 0, "unknown")
        .await;
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .unwrap();
    let evidence = ledger.criterion(1).unwrap().evidence.as_ref().unwrap();
    assert_eq!(
        (
            evidence.outcome,
            evidence.detail.clone(),
            evidence.workspace_generation,
            ledger.workspace_generation
        ),
        (EvidenceOutcome::Passed, None, 0, 0)
    );
}

#[tokio::test]
async fn w6_generated_cache_does_not_invalidate_a_passing_walk_check() {
    let fixture = fixture("walk-cache").await;
    let root = fixture.project.path();
    fixture
        .start("cache", "pytest -q tests/test_parse.py", root)
        .await;
    std::fs::create_dir(root.join("__pycache__")).unwrap();
    std::fs::write(root.join("__pycache__/check.pyc"), "cache").unwrap();
    fixture
        .finish(
            "cache",
            "pytest -q tests/test_parse.py",
            root,
            "completed",
            0,
            "unknown",
        )
        .await;
    assert_eq!(fixture.latest().await, (Some(EvidenceOutcome::Passed), 0));
}

#[tokio::test]
async fn w6_an_excluded_pin_remains_governed() {
    let fixture = fixture("walk-excluded-pin").await;
    let root = fixture.project.path();
    std::fs::create_dir(root.join(".venv")).unwrap();
    std::fs::write(root.join(".venv/data"), "first").unwrap();
    fixture
        .store
        .revise_acceptance(
            &fixture.run_id,
            1,
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate {
                    artifacts: Some(vec![".venv/data".to_string()]),
                    ..TermsUpdate::default()
                },
            }],
            "excluded-pin",
        )
        .await
        .unwrap();
    fixture
        .start("pin", "pytest -q tests/test_parse.py", root)
        .await;
    std::fs::write(root.join(".venv/data"), "other").unwrap();
    fixture
        .finish(
            "pin",
            "pytest -q tests/test_parse.py",
            root,
            "completed",
            0,
            "unknown",
        )
        .await;
    let ledger = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .unwrap();
    let evidence = ledger.criterion(1).unwrap().evidence.as_ref().unwrap();
    assert_eq!(
        (evidence.outcome, evidence.detail.as_deref()),
        (
            EvidenceOutcome::Unavailable,
            Some("the pinned artifacts or checker were unreadable or changed while the check ran")
        )
    );
}
