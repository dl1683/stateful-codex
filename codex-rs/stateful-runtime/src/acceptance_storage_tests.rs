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
use crate::AcceptanceState;
use crate::ArtifactState;
use crate::CommandEvidence;
use crate::CriterionTerms;
use crate::EvidenceOutcome;
use crate::NewObligation;
use crate::NewStatefulRun;
use crate::ObligationPacket;
use crate::RequestSpan;
use crate::RunBudget;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::StatefulRunUpdate;
use crate::TermsUpdate;
use crate::WorkflowMode;
use crate::unmet_criteria;

const GOAL: &str = "Fix the parser. Write the summary to out/report.json. All tests must pass.";

async fn store_with_run(
    mode: WorkflowMode,
) -> (TempDir, SqliteConfig, StatefulRunStore, StatefulRunId) {
    let home = TempDir::new().expect("tempdir");
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = StatefulRunStore::open(&sqlite).await.expect("store opens");
    let id = StatefulRunId::parse("acceptance-run").expect("run id");
    store
        .create_run(
            id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: GOAL.to_string(),
                mode,
                budget: RunBudget {
                    max_continuations: 24,
                    max_elapsed_seconds: 14_400,
                },
            },
        )
        .await
        .expect("run created");
    (home, sqlite, store, id)
}

fn span(quote: &str) -> RequestSpan {
    let start = GOAL.find(quote).expect("quote is in the goal");
    RequestSpan {
        start,
        end: start + quote.len(),
    }
}

fn user_check(quote: &str, command: &str) -> AcceptanceChange {
    AcceptanceChange::Add {
        origin: AcceptanceOrigin::User,
        kind: AcceptanceKind::Check,
        statement: quote.to_string(),
        request_span: Some(span(quote)),
        terms: CriterionTerms {
            required: true,
            check_command: Some(command.to_string()),
            expected_observation: Some("exit 0 with every test passing".to_string()),
            ..CriterionTerms::default()
        },
    }
}

fn report_deliverable() -> AcceptanceChange {
    AcceptanceChange::Add {
        origin: AcceptanceOrigin::User,
        kind: AcceptanceKind::Deliverable,
        statement: "The summary is written to out/report.json.".to_string(),
        request_span: Some(span("Write the summary to out/report.json.")),
        terms: CriterionTerms {
            required: true,
            artifacts: vec!["out/report.json".to_string()],
            ..CriterionTerms::default()
        },
    }
}

fn observed(ledger: &AcceptanceLedger, ordinal: u32, exit_code: i32) -> CommandEvidence {
    let criterion = ledger.criterion(ordinal).expect("criterion exists");
    CommandEvidence {
        ordinal,
        criterion_revision: criterion.revision,
        outcome: if exit_code == 0 {
            EvidenceOutcome::Passed
        } else {
            EvidenceOutcome::Failed
        },
        command: criterion.check_command.clone().expect("check command"),
        exit_code: Some(exit_code),
        output_tail: "1 passed".to_string(),
        output_digest: "sha256:output".to_string(),
        artifact_digest: None,
        detail: None,
        source_id: "call-check".to_string(),
    }
}

fn commit(ledger: &AcceptanceLedger, artifacts: BTreeMap<u32, ArtifactState>) -> AcceptanceCommit {
    AcceptanceCommit {
        ledger_revision: ledger.revision,
        workspace_generation: ledger.workspace_generation,
        artifacts,
        omission_required: false,
        verification: None,
    }
}

fn completion(expected_revision: u64) -> StatefulRunUpdate {
    StatefulRunUpdate {
        expected_revision,
        status: StatefulRunStatus::Completed,
        strategy: None,
        result: Some("Done.".to_string()),
    }
}

#[tokio::test]
async fn acceptance_ledger_persists_across_restart_with_spans_and_evidence() {
    let (_home, sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            /*expected_revision*/ 0,
            vec![
                user_check("All tests must pass.", "pytest -q"),
                report_deliverable(),
            ],
            "call-1",
        )
        .await
        .expect("criteria recorded");
    let quote = ledger.criteria[0]
        .request_span
        .and_then(|span| span.quote(GOAL).map(str::to_string));
    assert_eq!(quote.as_deref(), Some("All tests must pass."));
    store
        .record_command_evidence(&id, vec![observed(&ledger, 1, 0)])
        .await
        .expect("evidence recorded");
    let before = store.acceptance_ledger(&id).await.expect("ledger reads");
    assert_eq!(before.revision, 2);
    store.pool.close().await;

    let reopened = StatefulRunStore::open(&sqlite)
        .await
        .expect("store reopens");
    assert_eq!(
        reopened.acceptance_ledger(&id).await.expect("ledger reads"),
        before
    );
}

#[tokio::test]
async fn acceptance_revisions_conflict_and_user_criteria_cannot_be_weakened() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![
                user_check("All tests must pass.", "pytest -q"),
                report_deliverable(),
            ],
            "call-1",
        )
        .await
        .expect("criteria recorded");
    let conflict = store
        .revise_acceptance(
            &id,
            0,
            vec![user_check("Fix the parser.", "pytest -q tests/parser")],
            "call-2",
        )
        .await
        .expect_err("stale revision refused");
    assert!(matches!(
        conflict,
        StatefulRunStoreError::AcceptanceRevisionConflict {
            expected: 0,
            actual: 1
        }
    ));
    store
        .record_command_evidence(&id, vec![observed(&ledger, 1, 1)])
        .await
        .expect("failed check recorded");
    let before = store.acceptance_ledger(&id).await.expect("ledger reads");
    for change in [
        AcceptanceChange::Refine {
            ordinal: 1,
            statement: Some("Most tests pass.".to_string()),
            required: None,
            terms: TermsUpdate::default(),
        },
        AcceptanceChange::Refine {
            ordinal: 1,
            statement: None,
            required: None,
            terms: TermsUpdate {
                check_command: Some("true".to_string()),
                ..TermsUpdate::default()
            },
        },
        AcceptanceChange::Refine {
            ordinal: 1,
            statement: None,
            required: Some(false),
            terms: TermsUpdate::default(),
        },
        AcceptanceChange::Refine {
            ordinal: 2,
            statement: None,
            required: None,
            terms: TermsUpdate {
                artifacts: Some(Vec::new()),
                ..TermsUpdate::default()
            },
        },
        AcceptanceChange::Retire {
            ordinal: 2,
            reason: "Not needed.".to_string(),
        },
        AcceptanceChange::NoCheck {
            ordinal: 1,
            reason: "Too slow.".to_string(),
        },
        AcceptanceChange::Add {
            origin: AcceptanceOrigin::Omission,
            kind: AcceptanceKind::Constraint,
            statement: "Self-proposed.".to_string(),
            request_span: Some(span("Fix the parser.")),
            terms: CriterionTerms::default(),
        },
    ] {
        let error = store
            .revise_acceptance(&id, before.revision, vec![change], "call-3")
            .await
            .expect_err("weakening refused");
        assert!(
            matches!(
                error,
                StatefulRunStoreError::Acceptance(AcceptanceError::Refused(_))
            ),
            "{error}"
        );
        assert_eq!(
            store.acceptance_ledger(&id).await.expect("ledger reads"),
            before
        );
    }
    let unsupported = store
        .revise_acceptance(
            &id,
            before.revision,
            vec![user_check("Fix the parser.", "pytest\nrm -rf out")],
            "call-4",
        )
        .await
        .expect_err("multi-line check refused");
    assert!(matches!(
        unsupported,
        StatefulRunStoreError::Acceptance(AcceptanceError::UnsupportedCheckCommand)
    ));
}

#[tokio::test]
async fn acceptance_gate_rejects_stale_false_green_and_failed_checks_atomically() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![
                user_check("All tests must pass.", "pytest -q"),
                report_deliverable(),
            ],
            "call-1",
        )
        .await
        .expect("criteria recorded");
    store
        .record_command_evidence(&id, vec![observed(&ledger, 1, 0)])
        .await
        .expect("green check recorded");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    // Green pytest does not certify the separate output-file gate.
    let missing = BTreeMap::from([(
        2,
        ArtifactState::Observed {
            digest: "sha256:none".to_string(),
            missing: vec!["out/report.json".to_string()],
        },
    )]);
    assert_eq!(
        unmet_criteria(&ledger, &commit(&ledger, missing.clone())),
        vec![(
            2,
            "declared artifact out/report.json is missing".to_string()
        )]
    );
    let run = store
        .get_run(&id)
        .await
        .expect("run reads")
        .expect("run exists");
    let obligation = NewObligation {
        project_id: "project-1".to_string(),
        run_id: id.clone(),
        packet: ObligationPacket {
            learning: vec!["Learned.".to_string()],
            ..Default::default()
        },
        provenance_source_id: "call-complete".to_string(),
    };
    let rejected = store
        .complete_run_with_acceptance(
            &id,
            completion(run.revision),
            &commit(&ledger, missing),
            Some(("obligation-1".to_string(), obligation.clone())),
        )
        .await
        .expect_err("missing artifact blocks completion");
    assert!(matches!(rejected, StatefulRunStoreError::AcceptanceGate(_)));
    assert_eq!(
        store.get_run(&id).await.expect("run reads"),
        Some(run.clone())
    );
    assert_eq!(
        store
            .latest_obligation(&id)
            .await
            .expect("obligations read"),
        None
    );

    // A later workspace mutation makes the green check stale; the old commit no longer matches.
    let present = BTreeMap::from([(
        2,
        ArtifactState::Observed {
            digest: "sha256:report".to_string(),
            missing: Vec::new(),
        },
    )]);
    let validated = commit(&ledger, present.clone());
    store
        .bump_workspace_generation(&id)
        .await
        .expect("mutation recorded");
    let changed = store
        .complete_run_with_acceptance(&id, completion(run.revision), &validated, None)
        .await
        .expect_err("ledger changed under the commit");
    assert!(matches!(changed, StatefulRunStoreError::AcceptanceChanged));
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    assert_eq!(
        unmet_criteria(&ledger, &commit(&ledger, present.clone())),
        vec![(
            1,
            "stale: the workspace changed after `pytest -q` passed; run it again".to_string()
        )]
    );

    // A failed rerun stays failed; a passing rerun completes with the obligation atomically.
    store
        .record_command_evidence(&id, vec![observed(&ledger, 1, 2)])
        .await
        .expect("failed check recorded");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    assert_eq!(
        unmet_criteria(&ledger, &commit(&ledger, present.clone())),
        vec![(
            1,
            "`pytest -q` failed (exit 2); repair the work and run it again".to_string()
        )]
    );
    store
        .record_command_evidence(&id, vec![observed(&ledger, 1, 0)])
        .await
        .expect("green check recorded");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    let (completed, stored_obligation) = store
        .complete_run_with_acceptance(
            &id,
            completion(run.revision),
            &commit(&ledger, present),
            Some(("obligation-1".to_string(), obligation)),
        )
        .await
        .expect("current evidence completes");
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    assert!(stored_obligation.is_some());
}

#[tokio::test]
async fn acceptance_omission_proposals_block_until_reviewed_and_run_once() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .record_omission_check(
            &id,
            vec![(
                "Write the summary to out/report.json.".to_string(),
                span("Write the summary to out/report.json."),
            )],
        )
        .await
        .expect("omission recorded");
    assert!(ledger.omission_checked);
    assert_eq!(ledger.criteria[0].state, AcceptanceState::Proposed);
    assert_eq!(ledger.criteria[0].origin, AcceptanceOrigin::Omission);
    let again = store
        .record_omission_check(
            &id,
            vec![("Fix the parser.".to_string(), span("Fix the parser."))],
        )
        .await
        .expect("second check is a no-op");
    assert_eq!(again, ledger);
    let run = store
        .get_run(&id)
        .await
        .expect("run reads")
        .expect("run exists");
    assert!(matches!(
        store.update_run(&id, completion(run.revision)).await,
        Err(StatefulRunStoreError::AcceptanceGate(_))
    ));
    let reviewed = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Dismiss {
                ordinal: 1,
                reason: "The user withdrew the report in steering.".to_string(),
            }],
            "call-review",
        )
        .await
        .expect("proposal dismissed");
    assert_eq!(reviewed.criteria[0].state, AcceptanceState::Dismissed);
    let completed = store
        .update_run(&id, completion(run.revision))
        .await
        .expect("dismissed proposals do not gate");
    assert_eq!(completed.status, StatefulRunStatus::Completed);
}

#[tokio::test]
async fn acceptance_evidence_is_bounded_and_waits_for_socratic_execution() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Socratic).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![user_check("All tests must pass.", "pytest -q")],
            "call-1",
        )
        .await
        .expect("criteria can be agreed while pending");
    assert_eq!(
        store
            .record_command_evidence(&id, vec![observed(&ledger, 1, 0)])
            .await
            .expect("pending run skips evidence"),
        0
    );
    let manual = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Manual,
                statement: "The chart reads well.".to_string(),
                request_span: None,
                terms: CriterionTerms::default(),
            }],
            "call-2",
        )
        .await
        .expect("derived criterion added");
    let refused = store
        .revise_acceptance(
            &id,
            manual.revision,
            vec![AcceptanceChange::Observe {
                ordinal: 2,
                observation: "Looked at it.".to_string(),
            }],
            "call-3",
        )
        .await
        .expect_err("pending run records no evidence");
    assert!(matches!(
        refused,
        StatefulRunStoreError::Acceptance(AcceptanceError::Refused(_))
    ));

    store
        .update_mode(
            &id,
            crate::StatefulRunModeUpdate {
                expected_revision: 1,
                mode: WorkflowMode::Autonomous,
            },
        )
        .await
        .expect("user transitions to execution");
    for _ in 0..6 {
        store
            .record_command_evidence(&id, vec![observed(&manual, 1, 1)])
            .await
            .expect("evidence recorded");
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stateful_acceptance_evidence")
        .fetch_one(&store.pool)
        .await
        .expect("count reads");
    assert_eq!(rows, 4);
}

#[tokio::test]
async fn acceptance_rejected_completions_count_only_without_ledger_progress() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    assert_eq!(
        store.note_rejected_completion(&id).await.expect("counted"),
        1
    );
    assert_eq!(
        store.note_rejected_completion(&id).await.expect("counted"),
        2
    );
    store
        .revise_acceptance(
            &id,
            0,
            vec![user_check("All tests must pass.", "pytest -q")],
            "call-1",
        )
        .await
        .expect("ledger progress");
    assert_eq!(
        store.note_rejected_completion(&id).await.expect("counted"),
        1
    );
}

#[tokio::test]
async fn acceptance_criteria_carry_stable_ids_requirements_and_revisions() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let missing_expectation = store
        .revise_acceptance(
            &id,
            0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Check,
                statement: "Lint is clean.".to_string(),
                request_span: None,
                terms: CriterionTerms {
                    required: true,
                    check_command: Some("ruff check".to_string()),
                    ..CriterionTerms::default()
                },
            }],
            "call-0",
        )
        .await
        .expect_err("a check must state its expected observation");
    assert!(matches!(
        missing_expectation,
        StatefulRunStoreError::Acceptance(AcceptanceError::CheckWithoutExpectation)
    ));
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![
                report_deliverable(),
                AcceptanceChange::Add {
                    origin: AcceptanceOrigin::Derived,
                    kind: AcceptanceKind::Check,
                    statement: "The report totals reconcile with the inputs.".to_string(),
                    request_span: None,
                    terms: CriterionTerms {
                        required: true,
                        depends_on: vec![1],
                        milestone: Some("publish the report".to_string()),
                        check_command: Some("python reconcile.py".to_string()),
                        expected_observation: Some(
                            "independently recomputed totals equal the report".to_string(),
                        ),
                        ..CriterionTerms::default()
                    },
                },
            ],
            "call-1",
        )
        .await
        .expect("criteria recorded");
    let user = ledger.criterion(1).expect("C1");
    assert_eq!(
        (
            user.id.as_str(),
            user.requirement.as_str(),
            user.required,
            user.ledger_revision
        ),
        (
            "acceptance-run#C1",
            "Write the summary to out/report.json.",
            true,
            1
        )
    );
    let derived = ledger.criterion(2).expect("C2");
    assert_eq!(
        (
            derived.id.as_str(),
            derived.requirement.as_str(),
            derived.depends_on.clone(),
            derived.milestone.as_deref()
        ),
        (
            "acceptance-run#C2",
            "The report totals reconcile with the inputs.",
            vec![1],
            Some("publish the report")
        )
    );
    let forward = store
        .revise_acceptance(
            &id,
            1,
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate {
                    depends_on: Some(vec![2]),
                    ..TermsUpdate::default()
                },
            }],
            "call-2",
        )
        .await
        .expect_err("dependencies point only backwards");
    assert!(matches!(
        forward,
        StatefulRunStoreError::Acceptance(AcceptanceError::InvalidDependency)
    ));
    // C2's own check passes, but it depends on the missing report.
    store
        .record_command_evidence(&id, vec![observed(&ledger, 2, 0)])
        .await
        .expect("check recorded");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    let missing = BTreeMap::from([(
        1,
        ArtifactState::Observed {
            digest: "sha256:none".to_string(),
            missing: vec!["out/report.json".to_string()],
        },
    )]);
    assert_eq!(
        unmet_criteria(&ledger, &commit(&ledger, missing)),
        vec![
            (
                1,
                "declared artifact out/report.json is missing".to_string()
            ),
            (2, "depends on C1, which is unmet".to_string()),
        ]
    );
}

#[tokio::test]
async fn unverified_required_work_never_completes_but_optional_work_is_disclosed() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let manual = |required: bool, statement: &str| AcceptanceChange::Add {
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Manual,
        statement: statement.to_string(),
        request_span: None,
        terms: CriterionTerms {
            required,
            ..CriterionTerms::default()
        },
    };
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![
                manual(true, "The filing reconciles."),
                manual(false, "Performance is unchanged."),
            ],
            "call-1",
        )
        .await
        .expect("criteria recorded");
    let ledger = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![
                AcceptanceChange::NoCheck {
                    ordinal: 1,
                    reason: "No reconciliation source is available.".to_string(),
                },
                AcceptanceChange::NoCheck {
                    ordinal: 2,
                    reason: "No benchmark harness exists.".to_string(),
                },
            ],
            "call-2",
        )
        .await
        .expect("statements recorded");
    let unmet = unmet_criteria(&ledger, &commit(&ledger, BTreeMap::new()));
    assert_eq!(unmet.len(), 1);
    assert_eq!(unmet[0].0, 1);
    assert!(
        unmet[0]
            .1
            .starts_with("required but unverified (No reconciliation source is available.)")
    );
    let run = store
        .get_run(&id)
        .await
        .expect("run reads")
        .expect("run exists");
    assert!(matches!(
        store.update_run(&id, completion(run.revision)).await,
        Err(StatefulRunStoreError::AcceptanceGate(_))
    ));
    // The honest outcome is Blocked with a partial result.
    let blocked = store
        .update_run(
            &id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Blocked,
                strategy: None,
                result: Some("Partial: the filing could not be reconciled.".to_string()),
            },
        )
        .await
        .expect("blocked with a partial result");
    assert_eq!(blocked.status, StatefulRunStatus::Blocked);
}

#[tokio::test]
async fn completion_consumes_a_held_verification_lease_and_the_run_stays_running() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let attempt = store
        .begin_verification(&id, "owner-a", 60_000)
        .await
        .expect("attempt starts");
    assert_eq!(attempt, 1);
    let leased = store
        .begin_verification(&id, "owner-b", 60_000)
        .await
        .expect_err("one attempt at a time");
    assert!(matches!(
        leased,
        StatefulRunStoreError::Acceptance(AcceptanceError::VerificationLeased(_))
    ));
    let run = store
        .get_run(&id)
        .await
        .expect("run reads")
        .expect("run exists");
    assert_eq!(run.status, StatefulRunStatus::Running);
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    let mut stale = commit(&ledger, BTreeMap::new());
    stale.verification = Some(("owner-b".to_string(), attempt));
    assert!(matches!(
        store
            .complete_run_with_acceptance(&id, completion(run.revision), &stale, None)
            .await,
        Err(StatefulRunStoreError::AcceptanceChanged)
    ));
    store
        .end_verification(&id, "owner-a", attempt)
        .await
        .expect("refusal ends the attempt");
    let attempt = store
        .begin_verification(&id, "owner-b", 60_000)
        .await
        .expect("next attempt starts");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    let mut current = commit(&ledger, BTreeMap::new());
    current.verification = Some(("owner-b".to_string(), attempt));
    let (completed, _) = store
        .complete_run_with_acceptance(&id, completion(run.revision), &current, None)
        .await
        .expect("held lease completes");
    assert_eq!(completed.status, StatefulRunStatus::Completed);
    assert_eq!(
        store
            .acceptance_ledger(&id)
            .await
            .expect("ledger reads")
            .verification_lease_expires_at_ms,
        None
    );
}
