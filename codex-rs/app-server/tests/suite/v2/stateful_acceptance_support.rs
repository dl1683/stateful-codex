//! Shared acceptance setup for tests whose subject is not acceptance itself: every completed
//! run needs a criterion covering its request that is settled by a current receipt of its
//! host-admitted check plan. The setup pins the whole project root (an output and its
//! checker), admits the plan before the run's first turn, and the model then runs the check.

use std::path::Path;

use anyhow::Result;
use codex_state::SqliteConfig;
use codex_stateful_runtime::AcceptanceChange;
use codex_stateful_runtime::AcceptanceKind;
use codex_stateful_runtime::AcceptanceOrigin;
use codex_stateful_runtime::CriterionTerms;
use codex_stateful_runtime::RequestSpan;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStore;
use codex_utils_absolute_path::test_support::PathExt;
use core_test_support::responses;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;

const OUTPUT: &str = "accepted.txt";
const CHECKER: &str = "verify-acceptance.sh";
const CHECKER_SCRIPT: &str = "test -s accepted.txt\n";
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(super) const CHECK_COMMAND: &str = "sh verify-acceptance.sh";

/// Writes the pinned output and its checker into `root`, which must hold no other files.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(super) fn write_acceptance_files(root: &Path) -> Result<()> {
    std::fs::write(root.join(OUTPUT), "accepted\n")?;
    std::fs::write(root.join(CHECKER), CHECKER_SCRIPT)?;
    init_repository(root)
}

/// Makes `root` a git work tree: the host identifies a check's workspace only through git.
pub(super) fn init_repository(root: &Path) -> Result<()> {
    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(root)
        .status()?;
    anyhow::ensure!(status.success(), "git init failed");
    Ok(())
}

/// Adds C1 quoting the run's whole request and admits its check plan, with the checker
/// digest the host computes. `other_files` are the root's remaining project files, pinned so
/// the check sees a fully pinned workspace. Seed it before the run's first check: no evidence
/// exists yet.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(super) async fn seed_admitted_plan(
    codex_home: &Path,
    run_id: &str,
    other_files: &[&str],
) -> Result<()> {
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(codex_home.abs())).await?;
    let run_id = StatefulRunId::parse(run_id)?;
    let request = store.acceptance_request(&run_id).await?;
    let ledger = store.acceptance_ledger(&run_id).await?;
    let ledger = store
        .revise_acceptance(
            &run_id,
            ledger.revision,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::User,
                kind: AcceptanceKind::Deliverable,
                statement: "The request is answered.".to_string(),
                request_span: Some(RequestSpan {
                    start: 0,
                    end: request.len(),
                }),
                terms: CriterionTerms {
                    required: true,
                    artifacts: std::iter::once(OUTPUT)
                        .chain(other_files.iter().copied())
                        .map(str::to_string)
                        .collect(),
                    checker: vec![CHECKER.to_string()],
                    check_command: Some(CHECK_COMMAND.to_string()),
                    expected_observation: Some("the answer file exists".to_string()),
                    ..CriterionTerms::default()
                },
            }],
            "seed-acceptance",
        )
        .await?;
    store
        .revise_acceptance(
            &run_id,
            ledger.revision,
            vec![AcceptanceChange::Admit {
                ordinal: 1,
                checker_digest: Some(checker_digest()),
            }],
            "seed-admission",
        )
        .await?;
    Ok(())
}

/// The host's digest of the checker file set, as the observer reads it.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn checker_digest() -> String {
    let file = format!("sha256:{:x}", Sha256::digest(CHECKER_SCRIPT.as_bytes()));
    let mut hasher = Sha256::new();
    for part in [CHECKER, file.as_str()] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

/// The model turn that runs the admitted check in `root`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(super) fn run_check(call_id: &str, root: &Path) -> String {
    responses::sse(vec![
        responses::ev_response_created(call_id),
        responses::ev_function_call(
            call_id,
            "exec_command",
            &json!({"cmd": CHECK_COMMAND, "workdir": root.to_string_lossy(), "yield_time_ms": 10_000})
                .to_string(),
        ),
        responses::ev_completed(call_id),
    ])
}

/// For a run completed directly through the runtime (no observer reads files): settles its
/// request once with a passing receipt of an admitted plan, starts a verification attempt
/// for `owner` and returns the matching commit.
pub(super) async fn seeded_commit(
    store: &StatefulRunStore,
    run_id: &StatefulRunId,
    owner: &str,
    validated_obligation_sequence: Option<u64>,
) -> Result<codex_stateful_runtime::AcceptanceCommit> {
    const SEEDED_ARTIFACT: &str = "sha256:seeded-artifact";
    const SEEDED_CHECKER: &str = "sha256:seeded-checker";
    if store.acceptance_ledger(run_id).await?.criteria.is_empty() {
        let request = store.acceptance_request(run_id).await?;
        let ledger = store.acceptance_ledger(run_id).await?;
        let ledger = store
            .revise_acceptance(
                run_id,
                ledger.revision,
                vec![
                    AcceptanceChange::Add {
                        origin: AcceptanceOrigin::User,
                        kind: AcceptanceKind::Deliverable,
                        statement: "The request is answered.".to_string(),
                        request_span: Some(RequestSpan {
                            start: 0,
                            end: request.len(),
                        }),
                        terms: CriterionTerms {
                            required: true,
                            artifacts: vec![OUTPUT.to_string()],
                            checker: vec![CHECKER.to_string()],
                            check_command: Some(CHECK_COMMAND.to_string()),
                            expected_observation: Some("the answer file exists".to_string()),
                            ..CriterionTerms::default()
                        },
                    },
                    AcceptanceChange::Admit {
                        ordinal: 1,
                        checker_digest: Some(SEEDED_CHECKER.to_string()),
                    },
                ],
                "seed-acceptance",
            )
            .await?;
        let criterion = ledger.criterion(1).expect("seeded criterion");
        store
            .record_command_evidence(
                run_id,
                vec![codex_stateful_runtime::CommandEvidence {
                    ordinal: 1,
                    criterion_revision: criterion.revision,
                    outcome: codex_stateful_runtime::EvidenceOutcome::Passed,
                    command: CHECK_COMMAND.to_string(),
                    exit_code: Some(0),
                    output_tail: String::new(),
                    output_digest: "sha256:seeded-output".to_string(),
                    artifact_digest: Some(SEEDED_ARTIFACT.to_string()),
                    checker_digest: Some(SEEDED_CHECKER.to_string()),
                    detail: None,
                    start_generation: ledger.workspace_generation,
                    source_id: "seed-check".to_string(),
                }],
            )
            .await?;
    }
    let attempt = store.begin_verification(run_id, owner, 60_000).await?;
    let ledger = store.acceptance_ledger(run_id).await?;
    let observed = |digest: &str| {
        std::collections::BTreeMap::from([(
            1,
            codex_stateful_runtime::ArtifactState::Observed {
                digest: digest.to_string(),
                missing: Vec::new(),
            },
        )])
    };
    Ok(codex_stateful_runtime::AcceptanceCommit {
        ledger_revision: ledger.revision,
        workspace_generation: ledger.workspace_generation,
        artifacts: observed(SEEDED_ARTIFACT),
        checkers: observed(SEEDED_CHECKER),
        verification: codex_stateful_runtime::VerificationClaim {
            owner: owner.to_string(),
            attempt,
        },
        validated_obligation_sequence,
    })
}
