use std::path::Path;

use codex_stateful_runtime::AcceptanceChange;
use codex_stateful_runtime::ArtifactState;
use codex_stateful_runtime::EvidenceOutcome;
use codex_stateful_runtime::TermsUpdate;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::Fixture;
use super::fixture;
use crate::acceptance_observation::checker_states;

#[cfg(windows)]
const PYTHON: &str = "python";
#[cfg(not(windows))]
const PYTHON: &str = "python3";

fn directory_alias(target: &Path, alias: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, alias).expect("directory symlink");
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(alias.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .expect("junction preflight");
        assert!(output.status.success(), "junction preflight: {output:?}");
    }
    assert_eq!(
        alias.canonicalize().expect("alias"),
        target.canonicalize().expect("target")
    );
}

async fn declare(
    fixture: &Fixture,
    cwd: &str,
    checker: &str,
) -> codex_stateful_runtime::AcceptanceLedger {
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
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate {
                    checker: Some(vec![checker.to_string()]),
                    check_command: Some(format!("{PYTHON} check.py")),
                    check_cwd: Some(cwd.to_string()),
                    ..TermsUpdate::default()
                },
            }],
            "call-cwd",
            Some(fixture.project.path().canonicalize().expect("root")),
        )
        .await
        .expect("declared")
}

#[tokio::test]
async fn absolute_and_aliased_directories_admit_execute_and_match_only_the_canonical_directory() {
    let preflight = std::process::Command::new(PYTHON)
        .arg("--version")
        .output()
        .expect("python preflight");
    assert!(
        preflight.status.success(),
        "python preflight: {preflight:?}"
    );
    for spelling in ["absolute", "relative-alias", "absolute-alias"] {
        let fixture = fixture(&format!("cwd-{spelling}")).await;
        let root = fixture.project.path();
        std::fs::create_dir(root.join("real")).expect("real");
        std::fs::create_dir(root.join("other")).expect("other");
        std::fs::create_dir(root.join(".venv")).expect("excluded directory");
        std::fs::write(root.join(".gitignore"), ".venv/\n").expect("ignore alias for git identity");
        std::fs::write(root.join("real/check.py"), "print('OK')\n").expect("checker");
        directory_alias(&root.join("real"), &root.join(".venv/alias"));
        let alias = root.join(".venv/alias");
        let (cwd, checker, ran_in) = match spelling {
            "absolute" => (
                root.join("real").to_str().expect("UTF-8").to_string(),
                "real/check.py",
                root.join("real"),
            ),
            "relative-alias" => (
                ".venv/alias".to_string(),
                ".venv/alias/check.py",
                alias.clone(),
            ),
            "absolute-alias" => (
                alias.to_str().expect("UTF-8").to_string(),
                ".venv/alias/check.py",
                alias,
            ),
            _ => unreachable!(),
        };
        let ledger = declare(&fixture, &cwd, checker).await;
        assert_eq!(
            (
                ledger.criteria[0].check_cwd.as_deref(),
                ledger.criteria[0].checker.clone()
            ),
            (Some("real"), vec![checker.to_string()])
        );
        let states = checker_states(&fixture.roots, std::iter::once(&ledger.criteria[0])).await;
        let Some(ArtifactState::Observed { digest, missing }) = states.get(&1) else {
            panic!("readable checker: {states:?}");
        };
        assert!(missing.is_empty());
        let admitted = fixture
            .store
            .revise_acceptance(
                &fixture.run_id,
                ledger.revision,
                vec![AcceptanceChange::Admit {
                    ordinal: 1,
                    checker_digest: Some(digest.clone()),
                }],
                "call-admit",
                Some(root.canonicalize().expect("root")),
            )
            .await
            .expect("canonical target admission");
        let command = format!("{PYTHON} check.py");
        fixture.start("check", &command, &ran_in).await;
        let output = std::process::Command::new(PYTHON)
            .arg("check.py")
            .current_dir(&ran_in)
            .output()
            .expect("actual checker execution");
        assert!(output.status.success(), "{spelling}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8").trim(),
            "OK"
        );
        fixture
            .finish(
                "check",
                &command,
                &ran_in,
                "completed",
                /*exit_code*/ 0,
                "unknown",
            )
            .await;
        let passed = fixture
            .store
            .acceptance_ledger(&fixture.run_id)
            .await
            .expect("passed");
        let evidence = passed.criteria[0].evidence.as_ref().expect("host receipt");
        assert_eq!(
            (
                evidence.outcome,
                evidence.exit_code,
                evidence.detail.as_deref()
            ),
            (EvidenceOutcome::Passed, Some(0), None)
        );
        assert_eq!(passed.criteria[0].checker, admitted.criteria[0].checker);
        fixture
            .start("wrong-cwd", &command, &root.join("other"))
            .await;
        fixture
            .finish(
                "wrong-cwd",
                &command,
                &root.join("other"),
                "completed",
                /*exit_code*/ 0,
                "unknown",
            )
            .await;
        let after = fixture
            .store
            .acceptance_ledger(&fixture.run_id)
            .await
            .expect("after unmatched command");
        assert_eq!(after.criteria[0].evidence, passed.criteria[0].evidence);
    }
}

#[tokio::test]
async fn escaping_directory_alias_refuses_declaration_without_changing_the_ledger() {
    let fixture = fixture("cwd-escape").await;
    let outside = TempDir::new().expect("outside");
    directory_alias(outside.path(), &fixture.project.path().join("escape"));
    let before = fixture
        .store
        .acceptance_ledger(&fixture.run_id)
        .await
        .expect("before");
    let error = fixture
        .store
        .revise_acceptance(
            &fixture.run_id,
            before.revision,
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate {
                    check_cwd: Some("escape".to_string()),
                    ..TermsUpdate::default()
                },
            }],
            "call-escape",
            Some(fixture.project.path().canonicalize().expect("root")),
        )
        .await
        .expect_err("outside target");
    assert!(
        error
            .to_string()
            .contains("checkCwd must be inside the first project root"),
        "{error}"
    );
    assert_eq!(
        fixture
            .store
            .acceptance_ledger(&fixture.run_id)
            .await
            .expect("after"),
        before
    );
}
