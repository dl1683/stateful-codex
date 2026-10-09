use std::collections::BTreeMap;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::AcceptanceChange;
use crate::AcceptanceCommit;
use crate::AcceptanceError;
use crate::AcceptanceKind;
use crate::AcceptanceLedger;
use crate::AcceptanceOrigin;
use crate::ArtifactState;
use crate::CommandEvidence;
use crate::CriterionTerms;
use crate::CriterionVerdict;
use crate::EvidenceOutcome;
use crate::NewStatefulRun;
use crate::NewSteeringInstruction;
use crate::RequestSpan;
use crate::RunBudget;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::StatefulRunUpdate;
use crate::SteeringId;
use crate::SteeringStatus;
use crate::SteeringUpdate;
use crate::TermsUpdate;
use crate::VerificationClaim;
use crate::WorkflowMode;
use crate::ledger_verdicts;

const GOAL: &str = "Write out.txt.";
const PINNED: &str = "sha256:out";
const CHECKER: &str = "sha256:checker";

async fn store(goal: &str, mode: WorkflowMode) -> (TempDir, StatefulRunStore, StatefulRunId) {
    let home = TempDir::new().expect("tempdir");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens");
    let id = StatefulRunId::parse("plan-run").expect("run id");
    store
        .create_run(
            id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: goal.to_string(),
                mode,
                budget: RunBudget {
                    max_continuations: 24,
                    max_elapsed_seconds: 14_400,
                },
            },
        )
        .await
        .expect("run created");
    (home, store, id)
}

fn checked(command: &str, checker: &[&str]) -> AcceptanceChange {
    AcceptanceChange::Add {
        origin: AcceptanceOrigin::User,
        kind: AcceptanceKind::Deliverable,
        statement: "out.txt is written.".to_string(),
        request_span: Some(RequestSpan {
            start: 0,
            end: GOAL.len(),
        }),
        terms: CriterionTerms {
            required: true,
            artifacts: vec!["out.txt".to_string()],
            checker: checker.iter().map(ToString::to_string).collect(),
            check_command: Some(command.to_string()),
            expected_observation: Some("verify.sh exits 0 when out.txt is correct".to_string()),
            ..CriterionTerms::default()
        },
    }
}

fn admit(ordinal: u32) -> AcceptanceChange {
    AcceptanceChange::Admit {
        ordinal,
        checker_digest: Some(CHECKER.to_string()),
    }
}

fn receipt(ledger: &AcceptanceLedger, exit_code: i32, checker: Option<&str>) -> CommandEvidence {
    receipt_of(ledger, 1, exit_code, checker)
}

fn receipt_of(
    ledger: &AcceptanceLedger,
    ordinal: u32,
    exit_code: i32,
    checker: Option<&str>,
) -> CommandEvidence {
    let criterion = ledger.criterion(ordinal).expect("criterion");
    CommandEvidence {
        ordinal,
        criterion_revision: criterion.revision,
        outcome: if exit_code == 0 {
            EvidenceOutcome::Passed
        } else {
            EvidenceOutcome::Failed
        },
        command: criterion.check_command.clone().expect("check"),
        exit_code: Some(exit_code),
        output_tail: String::new(),
        output_digest: "sha256:output".to_string(),
        artifact_digest: Some(PINNED.to_string()),
        checker_digest: checker.map(str::to_string),
        detail: None,
        start_generation: ledger.workspace_generation,
        source_id: "call".to_string(),
    }
}

/// The same observed digest for the first few criteria.
fn state(digest: &str) -> BTreeMap<u32, ArtifactState> {
    (1..=4)
        .map(|ordinal| {
            (
                ordinal,
                ArtifactState::Observed {
                    digest: digest.to_string(),
                    missing: Vec::new(),
                },
            )
        })
        .collect()
}

async fn complete(
    store: &StatefulRunStore,
    id: &StatefulRunId,
    checkers: BTreeMap<u32, ArtifactState>,
) -> Result<crate::StatefulRun, StatefulRunStoreError> {
    let run = store.get_run(id).await?.expect("run");
    let attempt = store.begin_verification(id, "owner", 60_000).await?;
    let ledger = store.acceptance_ledger(id).await?;
    store
        .complete_run_with_acceptance(
            id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Completed,
                strategy: run.strategy.clone(),
                result: Some("Done.".to_string()),
            },
            &AcceptanceCommit {
                ledger_revision: ledger.revision,
                workspace_generation: ledger.workspace_generation,
                artifacts: state(PINNED),
                checkers,
                verification: VerificationClaim {
                    owner: "owner".to_string(),
                    attempt,
                },
                validated_obligation_sequence: None,
            },
            None,
        )
        .await
        .map(|(run, _)| run)
}

#[test]
fn only_an_execution_of_a_declared_checker_is_admissible() {
    let checker = ["verify.sh".to_string(), "tests/test_out.py".to_string()];
    for (command, cwd) in [
        ("sh verify.sh", None),
        ("bash ./verify.sh --strict", None),
        ("./verify.sh", None),
        ("python3 tests/test_out.py", None),
        ("python3.12 test_out.py", Some("tests")),
        ("../verify.sh", Some("tests")),
    ] {
        assert_eq!(
            crate::acceptance_plan::executed_checker(command, cwd, &checker),
            Ok(()),
            "{command}"
        );
    }
    for command in [
        "echo verify.sh",
        "cat verify.sh",
        "true # verify.sh",
        "sh -c verify.sh",
        "sh other/verify.sh",
        "sh verify.sh; true",
        "sh verify.sh || true",
        "FORCE=1 sh verify.sh",
        "sh /tmp/verify.sh",
        "sh /verify.sh",
        "./fake/sh verify.sh",
        "/bin/sh verify.sh",
        "bin/python3 tests/test_out.py",
        "sh ../verify.sh",
        "verify.sh",
        "pytest tests/test_out.py",
    ] {
        assert!(
            crate::acceptance_plan::executed_checker(command, None, &checker).is_err(),
            "{command}"
        );
    }
}

#[tokio::test]
async fn the_host_admits_only_plans_that_execute_a_declared_checker() {
    let (_home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![checked("echo suite-ok", &["verify.sh"])],
            "call-1",
        )
        .await
        .expect("criterion");
    for (change, why) in [
        (admit(1), "an echo runs no checker"),
        (
            AcceptanceChange::Admit {
                ordinal: 1,
                checker_digest: None,
            },
            "unreadable checker bytes",
        ),
    ] {
        let error = store
            .revise_acceptance(&id, ledger.revision, vec![change], "call-admit")
            .await
            .expect_err(why);
        assert!(
            matches!(
                error,
                StatefulRunStoreError::Acceptance(AcceptanceError::Refused(_))
            ),
            "{why}: {error}"
        );
    }
    assert!(matches!(
        store
            .revise_acceptance(
                &id,
                ledger.revision,
                vec![checked("sh verify.sh", &["out.txt"])],
                "call-overlap"
            )
            .await,
        Err(StatefulRunStoreError::Acceptance(
            AcceptanceError::InvalidChecker
        ))
    ));
    let refined = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![
                AcceptanceChange::Refine {
                    ordinal: 1,
                    statement: None,
                    required: None,
                    terms: TermsUpdate {
                        check_command: Some("sh verify.sh".to_string()),
                        ..TermsUpdate::default()
                    },
                },
                admit(1),
            ],
            "call-admit",
        )
        .await
        .expect("a check naming its checker is admitted");
    let plan = refined.criteria[0].plan.clone().expect("plan");
    assert_eq!(
        (plan.criterion_revision, plan.checker_digest.as_str()),
        (2, CHECKER)
    );
}

#[tokio::test]
async fn an_admitted_plan_settles_unattended_autonomous_work_without_steering() {
    let (_home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![checked("sh verify.sh", &["verify.sh"]), admit(1)],
            "call-1",
        )
        .await
        .expect("admitted");
    // A failing receipt keeps it unmet; repair and a passing receipt of the frozen checker
    // settle it.
    store
        .record_command_evidence(&id, vec![receipt(&ledger, 1, Some(CHECKER))])
        .await
        .expect("failed");
    assert!(complete(&store, &id, state(CHECKER)).await.is_err());
    store.bump_workspace_generation(&id).await.expect("repair");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    store
        .record_command_evidence(&id, vec![receipt(&ledger, 0, Some(CHECKER))])
        .await
        .expect("passed");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    assert_eq!(
        ledger_verdicts(&ledger, &state(PINNED), &state(CHECKER))[0].1,
        CriterionVerdict::SatisfiedByHost
    );
    // A changed checker, or a receipt that did not run the frozen bytes, is not current.
    assert!(
        ledger_verdicts(&ledger, &state(PINNED), &state("sha256:edited"))[0]
            .1
            .is_unmet()
    );
    assert_eq!(
        complete(&store, &id, state(CHECKER))
            .await
            .expect("completes unattended")
            .status,
        StatefulRunStatus::Completed
    );
}

#[tokio::test]
async fn changing_the_plan_or_receipts_of_other_bytes_need_readmission() {
    let (_home, store, id) = store(GOAL, WorkflowMode::Collaborative).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![checked("sh verify.sh", &["verify.sh"]), admit(1)],
            "call-1",
        )
        .await
        .expect("admitted");
    store
        .record_command_evidence(&id, vec![receipt(&ledger, 0, Some("sha256:other"))])
        .await
        .expect("passed");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    assert!(
        ledger_verdicts(&ledger, &state(PINNED), &state(CHECKER))[0]
            .1
            .is_unmet()
    );
    let changed = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate {
                    checker: Some(vec!["verify.sh".to_string(), "fixtures.csv".to_string()]),
                    ..TermsUpdate::default()
                },
            }],
            "call-scope",
        )
        .await
        .expect("dependencies grew");
    let verdict = crate::criterion_verdict(
        &changed.criteria[0],
        changed.workspace_generation,
        None,
        None,
    );
    assert!(verdict.is_unmet());
    assert!(
        changed.criteria[0]
            .plan
            .as_ref()
            .is_some_and(|plan| plan.criterion_revision != changed.criteria[0].revision)
    );
}

#[tokio::test]
async fn applied_steering_blocks_until_reconciled_and_withdraws_admissions() {
    let (_home, store, id) = store(GOAL, WorkflowMode::Collaborative).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![checked("sh verify.sh", &["verify.sh"]), admit(1)],
            "call-1",
        )
        .await
        .expect("admitted");
    store
        .record_command_evidence(&id, vec![receipt(&ledger, 0, Some(CHECKER))])
        .await
        .expect("passed");
    let steering_id = SteeringId::parse("add-output").expect("id");
    let submitted = store
        .submit_steering(
            steering_id.clone(),
            NewSteeringInstruction {
                project_id: "project-1".to_string(),
                run_id: id.clone(),
                input: "Also write out2.txt.".to_string(),
                affected_obligation_ids: Vec::new(),
            },
        )
        .await
        .expect("steering");
    store
        .update_steering(
            &steering_id,
            SteeringUpdate {
                expected_revision: submitted.revision,
                status: SteeringStatus::Acknowledged,
                resulting_strategy_revision: None,
                reason: None,
            },
        )
        .await
        .expect("acknowledged");
    let run = store.get_run(&id).await.expect("run").expect("exists");
    let applied = store
        .update_run(
            &id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Running,
                strategy: Some("Write both outputs.".to_string()),
                result: None,
            },
        )
        .await
        .expect("strategy");
    store
        .update_steering(
            &steering_id,
            SteeringUpdate {
                expected_revision: submitted.revision + 1,
                status: SteeringStatus::Applied,
                resulting_strategy_revision: Some(applied.strategy_revision),
                reason: None,
            },
        )
        .await
        .expect("applied");
    let error = complete(&store, &id, state(CHECKER))
        .await
        .expect_err("old evidence after a scope change");
    assert!(
        error.to_string().contains("may have changed the scope"),
        "{error}"
    );
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    let not_incorporated = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::ReconcileSteering {
                steering_id: steering_id.to_string(),
                reason: "Adds out2.txt; covered by a new criterion.".to_string(),
            }],
            "call-reconcile",
        )
        .await
        .expect_err("a claimed criterion that does not exist incorporates nothing");
    assert!(
        not_incorporated
            .to_string()
            .contains("\"Also write out2.txt.\" is covered by no criterion"),
        "{not_incorporated}"
    );
    // Re-running the old check does not complete the changed scope.
    store
        .record_command_evidence(&id, vec![receipt(&ledger, 0, Some(CHECKER))])
        .await
        .expect("rerun");
    assert!(complete(&store, &id, state(CHECKER)).await.is_err());
    let request = store.acceptance_request(&id).await.expect("request");
    assert_eq!(request, "Write out.txt.\nAlso write out2.txt.");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    let start = GOAL.len() + 1;
    let reconciled = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![
                AcceptanceChange::Add {
                    origin: AcceptanceOrigin::User,
                    kind: AcceptanceKind::Deliverable,
                    statement: "out2.txt is written.".to_string(),
                    request_span: Some(RequestSpan {
                        start,
                        end: request.len(),
                    }),
                    terms: CriterionTerms {
                        required: true,
                        artifacts: vec!["out2.txt".to_string()],
                        checker: vec!["verify2.sh".to_string()],
                        check_command: Some("sh verify2.sh".to_string()),
                        expected_observation: Some("exit 0".to_string()),
                        ..CriterionTerms::default()
                    },
                },
                AcceptanceChange::ReconcileSteering {
                    steering_id: steering_id.to_string(),
                    reason: "Adds out2.txt as C2.".to_string(),
                },
            ],
            "call-reconcile",
        )
        .await
        .expect("incorporated and reconciled");
    assert_eq!(reconciled.criteria[0].plan, None);
    assert!(reconciled.workspace_generation > ledger.workspace_generation);
    let readmitted = store
        .revise_acceptance(
            &id,
            reconciled.revision,
            vec![admit(1), admit(2)],
            "call-readmit",
        )
        .await
        .expect("readmitted");
    store
        .record_command_evidence(&id, vec![receipt(&readmitted, 0, Some(CHECKER))])
        .await
        .expect("C1 passes again");
    let error = complete(&store, &id, state(CHECKER))
        .await
        .expect_err("the added output is unchecked");
    assert!(error.to_string().contains("C2"), "{error}");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    store
        .record_command_evidence(&id, vec![receipt_of(&ledger, 2, 0, Some(CHECKER))])
        .await
        .expect("C2 passes");
    assert_eq!(
        complete(&store, &id, state(CHECKER))
            .await
            .expect("both outputs verified")
            .status,
        StatefulRunStatus::Completed
    );
}

#[tokio::test]
async fn pending_commands_fence_completion_and_reentry_clears_them() {
    let (_home, store, id) = store("Answer a lookup.", WorkflowMode::Autonomous).await;
    store
        .begin_command(&id, "call-running")
        .await
        .expect("pending");
    let error = complete(&store, &id, BTreeMap::new())
        .await
        .expect_err("pending command");
    assert!(error.to_string().contains("have not finished"), "{error}");
    store.invalidate_after_reentry(&id).await.expect("reentry");
    assert_eq!(
        store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .pending_commands,
        0
    );
    store
        .begin_command(&id, "call-done")
        .await
        .expect("pending");
    store
        .finish_command(&id, "call-done")
        .await
        .expect("accounted");
    assert_eq!(
        store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .pending_commands,
        0
    );
}

#[tokio::test]
async fn an_exited_command_without_an_end_closes_as_terminated_unknown() {
    let (_home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![checked("sh verify.sh", &["verify.sh"]), admit(1)],
            "call-1",
        )
        .await
        .expect("admitted");
    store
        .record_command_evidence(&id, vec![receipt(&ledger, 0, Some(CHECKER))])
        .await
        .expect("passed");
    store
        .begin_command(&id, "call-denied")
        .await
        .expect("pending");
    assert!(
        store
            .command_pending(&id, "call-denied")
            .await
            .expect("read")
    );
    assert!(
        store
            .close_unaccounted_command(&id, "call-denied")
            .await
            .expect("closed")
    );
    // Its partial effects are unknown: nothing is pending, and the earlier pass is stale.
    let closed = store.acceptance_ledger(&id).await.expect("ledger");
    assert_eq!(
        (closed.pending_commands, closed.workspace_generation),
        (0, ledger.workspace_generation + 1)
    );
    assert!(
        ledger_verdicts(&closed, &state(PINNED), &state(CHECKER))[0]
            .1
            .is_unmet()
    );
    // A command already accounted for is not closed again.
    assert!(
        !store
            .close_unaccounted_command(&id, "call-denied")
            .await
            .expect("no-op")
    );
}

#[tokio::test]
async fn a_short_request_without_criteria_cannot_complete_after_an_action() {
    let (_home, store, id) = store("Answer a lookup.", WorkflowMode::Collaborative).await;
    store.record_host_action(&id).await.expect("action");
    let error = complete(&store, &id, BTreeMap::new())
        .await
        .expect_err("a run with an action owes coverage");
    assert!(
        error
            .to_string()
            .contains("1 request sentences are covered by no criterion"),
        "{error}"
    );
}

#[tokio::test]
async fn identical_labelled_evidence_is_not_progress() {
    let (_home, store, id) = store(GOAL, WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::User,
                kind: AcceptanceKind::Manual,
                statement: "out.txt is written.".to_string(),
                request_span: Some(RequestSpan {
                    start: 0,
                    end: GOAL.len(),
                }),
                terms: CriterionTerms {
                    required: true,
                    ..CriterionTerms::default()
                },
            }],
            "call-1",
        )
        .await
        .expect("criterion");
    let no_check = || AcceptanceChange::NoCheck {
        ordinal: 1,
        reason: "No checker exists.".to_string(),
    };
    let ledger = store
        .revise_acceptance(&id, ledger.revision, vec![no_check()], "call-2")
        .await
        .expect("first");
    for attempt in 1..=3 {
        assert_eq!(
            store.note_rejected_completion(&id).await.expect("counted"),
            attempt
        );
        let again = store
            .revise_acceptance(&id, ledger.revision, vec![no_check()], "call-again")
            .await
            .expect("identical restatement");
        assert_eq!(again.revision, ledger.revision);
    }
}
