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
    let criterion = ledger.criterion(1).expect("C1");
    CommandEvidence {
        ordinal: 1,
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

fn state(digest: &str) -> BTreeMap<u32, ArtifactState> {
    BTreeMap::from([(
        1,
        ArtifactState::Observed {
            digest: digest.to_string(),
            missing: Vec::new(),
        },
    )])
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

#[tokio::test]
async fn the_host_admits_only_plans_that_run_a_named_checker() {
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
        (admit(1), "an echo names no checker"),
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
    let reconciled = store
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
        .expect("reconciled");
    assert_eq!(reconciled.criteria[0].plan, None);
    assert!(reconciled.workspace_generation > ledger.workspace_generation);
    assert!(complete(&store, &id, state(CHECKER)).await.is_err());
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
    // A cheap lookup with nothing pending completes; the host records the admission.
    let ledger_before = store.acceptance_ledger(&id).await.expect("ledger");
    assert!(!ledger_before.cheap_lookup);
}

#[tokio::test]
async fn a_cheap_lookup_admission_is_recorded() {
    let (_home, store, id) = store("Answer a lookup.", WorkflowMode::Collaborative).await;
    complete(&store, &id, BTreeMap::new())
        .await
        .expect("completes");
    assert!(
        store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .cheap_lookup
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
