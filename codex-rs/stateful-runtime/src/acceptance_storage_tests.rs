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
use crate::CriterionVerdict;
use crate::DismissalReceipt;
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
use crate::VerificationClaim;
use crate::WorkflowMode;
use crate::ledger_verdicts;
use crate::unmet_criteria;

const GOAL: &str = "Fix the parser. Write the summary to out/report.json. All tests must pass.";
const DIGEST: &str = "sha256:pinned";

async fn store_with_goal(
    goal: &str,
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
    (home, sqlite, store, id)
}

async fn store_with_run(
    mode: WorkflowMode,
) -> (TempDir, SqliteConfig, StatefulRunStore, StatefulRunId) {
    store_with_goal(GOAL, mode).await
}

fn span_in(goal: &str, quote: &str) -> RequestSpan {
    let start = goal.find(quote).expect("quote is in the goal");
    RequestSpan {
        start,
        end: start + quote.len(),
    }
}

fn span(quote: &str) -> RequestSpan {
    span_in(GOAL, quote)
}

fn user_check(quote: &str, command: &str) -> AcceptanceChange {
    AcceptanceChange::Add {
        origin: AcceptanceOrigin::User,
        kind: AcceptanceKind::Check,
        statement: quote.to_string(),
        request_span: Some(span(quote)),
        terms: CriterionTerms {
            required: true,
            artifacts: vec!["tests/test_parser.py".to_string()],
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
        artifact_digest: Some(DIGEST.to_string()),
        checker_digest: None,
        detail: None,
        start_generation: ledger.workspace_generation,
        source_id: "call-check".to_string(),
    }
}

fn pinned(ordinals: &[u32]) -> BTreeMap<u32, ArtifactState> {
    ordinals
        .iter()
        .map(|ordinal| {
            (
                *ordinal,
                ArtifactState::Observed {
                    digest: DIGEST.to_string(),
                    missing: Vec::new(),
                },
            )
        })
        .collect()
}

fn completion(expected_revision: u64) -> StatefulRunUpdate {
    StatefulRunUpdate {
        expected_revision,
        status: StatefulRunStatus::Completed,
        strategy: None,
        result: Some("Done.".to_string()),
    }
}

/// The public terminal path: a leased verification decision consumed by the transaction.
async fn complete(
    store: &StatefulRunStore,
    id: &StatefulRunId,
    artifacts: BTreeMap<u32, ArtifactState>,
    obligation: Option<(String, NewObligation)>,
) -> Result<crate::StatefulRun, StatefulRunStoreError> {
    let run = store.get_run(id).await?.expect("run exists");
    let attempt = store.begin_verification(id, "test-owner", 60_000).await?;
    let ledger = store.acceptance_ledger(id).await?;
    let latest = store.latest_obligation(id).await?;
    store
        .complete_run_with_acceptance(
            id,
            completion(run.revision),
            &AcceptanceCommit {
                ledger_revision: ledger.revision,
                workspace_generation: ledger.workspace_generation,
                artifacts,
                checkers: BTreeMap::new(),
                verification: VerificationClaim {
                    owner: "test-owner".to_string(),
                    attempt,
                },
                validated_obligation_sequence: latest.map(|obligation| obligation.sequence),
            },
            obligation,
        )
        .await
        .map(|(run, _)| run)
}

/// Completes through a host-admitted plan that settles the whole request.
async fn complete_settled(store: &StatefulRunStore, id: &StatefulRunId) -> crate::StatefulRun {
    let run = store
        .get_run(id)
        .await
        .expect("run reads")
        .expect("run exists");
    let commit =
        crate::acceptance_test_support::settled_commit(store, id, "test-owner", None).await;
    store
        .complete_run_with_acceptance(id, completion(run.revision), &commit, None)
        .await
        .expect("settled request completes")
        .0
}

#[tokio::test]
async fn acceptance_ledger_persists_across_restart_with_spans_and_evidence() {
    let (_home, sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
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
    assert_eq!(
        ledger.criteria[0]
            .request_span
            .and_then(|span| span.quote(GOAL).map(str::to_string))
            .as_deref(),
        Some("All tests must pass.")
    );
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
async fn every_generic_terminal_writer_refuses_a_new_completed() {
    let (_home, _sqlite, store, id) =
        store_with_goal("Answer a lookup.", WorkflowMode::Collaborative).await;
    let run = store
        .get_run(&id)
        .await
        .expect("run reads")
        .expect("run exists");
    assert!(matches!(
        store.update_run(&id, completion(run.revision)).await,
        Err(StatefulRunStoreError::CompletionRequiresAcceptanceDecision)
    ));
    assert_eq!(store.get_run(&id).await.expect("run reads"), Some(run));
    // A settled request completes through the host decision.
    assert_eq!(
        complete_settled(&store, &id).await.status,
        StatefulRunStatus::Completed
    );
}

#[tokio::test]
async fn a_substantial_empty_ledger_cannot_complete_through_the_decision() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let error = complete(&store, &id, BTreeMap::new(), None)
        .await
        .expect_err("uncovered sentences gate");
    assert!(
        error
            .to_string()
            .contains("3 request sentences are covered by no criterion or proposal"),
        "{error}"
    );
}

#[tokio::test]
async fn user_criteria_cannot_be_weakened_and_revisions_conflict() {
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
    assert!(matches!(
        store
            .revise_acceptance(
                &id,
                0,
                vec![user_check("Fix the parser.", "pytest -q tests")],
                "call-2"
            )
            .await,
        Err(StatefulRunStoreError::AcceptanceRevisionConflict {
            expected: 0,
            actual: 1
        })
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
        AcceptanceChange::Observe {
            ordinal: 1,
            observation: "All tests pass.".to_string(),
            artifact_digest: None,
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
    assert!(matches!(
        store
            .revise_acceptance(
                &id,
                before.revision,
                vec![user_check("Fix the parser.", "pytest\nrm -rf out")],
                "call-4"
            )
            .await,
        Err(StatefulRunStoreError::Acceptance(
            AcceptanceError::UnsupportedCheckCommand
        ))
    ));
}

#[tokio::test]
async fn only_a_covering_user_criterion_dismisses_a_proposal() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let (ledger, remaining) = store.propose_uncovered(&id).await.expect("omission pass");
    assert_eq!((ledger.criteria.len(), remaining), (3, 0));
    // There is no quote-based waiver; a proposal does not cover another proposal.
    let refused = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Dismiss {
                ordinal: 2,
                reason: "The user withdrew the report in steering.".to_string(),
                receipt: DismissalReceipt::CoveredBy(1),
            }],
            "call-forged",
        )
        .await
        .expect_err("a proposal is no cover");
    assert!(
        matches!(
            refused,
            StatefulRunStoreError::Acceptance(AcceptanceError::Refused(_))
        ),
        "{refused}"
    );
    assert_eq!(
        store.acceptance_ledger(&id).await.expect("ledger reads"),
        ledger
    );
    // A user criterion quoting the sentence covers the proposal.
    let ledger = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![report_deliverable()],
            "call-cover",
        )
        .await
        .expect("covering criterion");
    let dismissed = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Dismiss {
                ordinal: 2,
                reason: "C4 already states this requirement.".to_string(),
                receipt: DismissalReceipt::CoveredBy(4),
            }],
            "call-receipt",
        )
        .await
        .expect("covered dismissal");
    assert_eq!(dismissed.criteria[1].state, AcceptanceState::Dismissed);
    // The receipt is re-validated: without its cover, the dismissal no longer counts.
    let mut uncovered = dismissed;
    uncovered
        .criteria
        .retain(|criterion| criterion.ordinal != 4);
    assert!(
        ledger_verdicts(&uncovered, &BTreeMap::new(), &BTreeMap::new())[1]
            .1
            .is_unmet()
    );
    assert_eq!(
        crate::uncovered_sentences(GOAL, &uncovered)
            .into_iter()
            .map(|span| span.quote(GOAL).expect("span").to_string())
            .collect::<Vec<_>>(),
        vec!["Write the summary to out/report.json.".to_string()]
    );
}
#[tokio::test]
async fn omission_overflow_persists_across_restart_and_blocks() {
    let goal = (1..=11)
        .map(|index| format!("Write output file number {index} to out/{index}.txt."))
        .collect::<Vec<_>>()
        .join(" ");
    let (_home, sqlite, store, id) = store_with_goal(&goal, WorkflowMode::Autonomous).await;
    store
        .record_side_effect(&id)
        .await
        .expect("the run wrote something");
    let (ledger, remaining) = store.propose_uncovered(&id).await.expect("first batch");
    assert_eq!((ledger.criteria.len(), remaining), (8, 3));
    store.pool.close().await;
    let store = StatefulRunStore::open(&sqlite)
        .await
        .expect("store reopens");
    let error = complete(&store, &id, BTreeMap::new(), None)
        .await
        .expect_err("overflow and proposals gate");
    assert!(
        error
            .to_string()
            .contains("3 request sentences are covered by no criterion or proposal"),
        "{error}"
    );
    let (ledger, remaining) = store.propose_uncovered(&id).await.expect("next batch");
    assert_eq!((ledger.criteria.len(), remaining), (11, 0));
}

#[tokio::test]
async fn a_full_ledger_keeps_the_remaining_sentences_uncovered() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let derived = |index: usize| AcceptanceChange::Add {
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Manual,
        statement: format!("Derived step {index}."),
        request_span: None,
        terms: CriterionTerms::default(),
    };
    let mut revision = 0;
    for batch in 0..4 {
        revision = store
            .revise_acceptance(
                &id,
                revision,
                (0..8).map(|index| derived(batch * 8 + index)).collect(),
                "call-fill",
            )
            .await
            .expect("filled")
            .revision;
    }
    let (ledger, remaining) = store.propose_uncovered(&id).await.expect("no room");
    assert_eq!((ledger.criteria.len(), remaining), (32, 3));
}

#[tokio::test]
async fn manual_assertions_and_file_presence_do_not_settle_requirements() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![
                AcceptanceChange::Add {
                    origin: AcceptanceOrigin::User,
                    kind: AcceptanceKind::Check,
                    statement: "All tests must pass.".to_string(),
                    request_span: Some(span("All tests must pass.")),
                    terms: CriterionTerms {
                        required: true,
                        ..CriterionTerms::default()
                    },
                },
                report_deliverable(),
                AcceptanceChange::Add {
                    origin: AcceptanceOrigin::Derived,
                    kind: AcceptanceKind::Existence,
                    statement: "The report file exists.".to_string(),
                    request_span: None,
                    terms: CriterionTerms {
                        required: true,
                        artifacts: vec!["out/report.json".to_string()],
                        ..CriterionTerms::default()
                    },
                },
            ],
            "call-1",
        )
        .await
        .expect("criteria recorded");
    // Bare prose cannot settle an executable user requirement.
    assert!(matches!(
        store
            .revise_acceptance(
                &id,
                ledger.revision,
                vec![AcceptanceChange::Observe {
                    ordinal: 1,
                    observation: "All tests pass.".to_string(),
                    artifact_digest: None,
                }],
                "call-assert",
            )
            .await,
        Err(StatefulRunStoreError::Acceptance(AcceptanceError::Refused(
            _
        )))
    ));
    let verdicts = ledger_verdicts(&ledger, &pinned(&[2, 3]), &BTreeMap::new());
    assert!(verdicts[0].1.is_unmet());
    // A present but content-bearing deliverable is not settled by existence.
    assert!(verdicts[1].1.is_unmet(), "{:?}", verdicts[1]);
    // An explicit derived existence predicate is.
    assert_eq!(verdicts[2].1, CriterionVerdict::ArtifactsPresent);
}

#[tokio::test]
async fn receipts_are_bound_to_start_state_pinned_content_and_reentry() {
    let (_home, _sqlite, store, id) =
        store_with_goal("All tests must pass.", WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Check,
                statement: "Unit tests pass.".to_string(),
                request_span: None,
                terms: CriterionTerms {
                    required: true,
                    artifacts: vec!["src/parse.py".to_string()],
                    check_command: Some("pytest -q".to_string()),
                    expected_observation: Some("exit 0".to_string()),
                    ..CriterionTerms::default()
                },
            }],
            "call-1",
        )
        .await
        .expect("criterion");
    // A mutation observed while the check ran: the receipt is unavailable, not passed.
    let raced = observed(&ledger, 1, 0);
    store
        .bump_workspace_generation(&id)
        .await
        .expect("mutation");
    store
        .record_command_evidence(&id, vec![raced])
        .await
        .expect("recorded");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    assert_eq!(
        ledger.criteria[0]
            .evidence
            .as_ref()
            .map(|evidence| evidence.outcome),
        Some(EvidenceOutcome::Unavailable)
    );
    // A clean receipt goes stale on content change and on cold re-entry.
    store
        .record_command_evidence(&id, vec![observed(&ledger, 1, 0)])
        .await
        .expect("recorded");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    let changed = BTreeMap::from([(
        1,
        ArtifactState::Observed {
            digest: "sha256:changed".to_string(),
            missing: Vec::new(),
        },
    )]);
    assert!(
        unmet_criteria(&ledger, &changed, &BTreeMap::new())[0]
            .1
            .contains("pinned artifacts changed")
    );
    store.invalidate_after_reentry(&id).await.expect("re-entry");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    assert!(
        unmet_criteria(&ledger, &pinned(&[1]), &BTreeMap::new())[0]
            .1
            .starts_with("stale: the workspace changed")
    );
}

#[tokio::test]
async fn manual_observations_are_pinned_to_the_inspected_content() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Manual,
                statement: "The chart reads well.".to_string(),
                request_span: None,
                terms: CriterionTerms {
                    required: true,
                    artifacts: vec!["out/chart.png".to_string()],
                    ..CriterionTerms::default()
                },
            }],
            "call-1",
        )
        .await
        .expect("criterion");
    let ledger = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Observe {
                ordinal: 1,
                observation: "Viewed the chart; axes are labelled.".to_string(),
                artifact_digest: Some(DIGEST.to_string()),
            }],
            "call-2",
        )
        .await
        .expect("observation");
    assert_eq!(
        ledger_verdicts(&ledger, &pinned(&[1]), &BTreeMap::new())[0].1,
        CriterionVerdict::ManualObservation
    );
    let replaced = BTreeMap::from([(
        1,
        ArtifactState::Observed {
            digest: "sha256:replaced".to_string(),
            missing: Vec::new(),
        },
    )]);
    assert!(
        ledger_verdicts(&ledger, &replaced, &BTreeMap::new())[0]
            .1
            .is_unmet()
    );
}

#[tokio::test]
async fn admitted_blockers_are_checked_inside_the_terminal_transaction() {
    let (_home, _sqlite, store, id) =
        store_with_goal("Answer a lookup.", WorkflowMode::Collaborative).await;
    let run = store.get_run(&id).await.expect("run").expect("exists");
    let attempt = store
        .begin_verification(&id, "test-owner", 60_000)
        .await
        .expect("attempt");
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    // A blocker appended after the caller validated "no obligation".
    store
        .append_obligation(
            "late-blocker".to_string(),
            NewObligation {
                project_id: "project-1".to_string(),
                run_id: id.clone(),
                packet: ObligationPacket {
                    blockers: vec!["Approval is absent.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "call-late".to_string(),
            },
        )
        .await
        .expect("blocker recorded");
    let commit = AcceptanceCommit {
        ledger_revision: ledger.revision,
        workspace_generation: ledger.workspace_generation,
        artifacts: BTreeMap::new(),
        checkers: BTreeMap::new(),
        verification: VerificationClaim {
            owner: "test-owner".to_string(),
            attempt,
        },
        validated_obligation_sequence: None,
    };
    assert!(matches!(
        store
            .complete_run_with_acceptance(&id, completion(run.revision), &commit, None)
            .await,
        Err(StatefulRunStoreError::AcceptanceChanged)
    ));
    let error = complete(&store, &id, BTreeMap::new(), None)
        .await
        .expect_err("recorded blocker gates");
    assert!(
        error.to_string().contains("declares open blockers"),
        "{error}"
    );
    let final_with_blocker = (
        "final".to_string(),
        NewObligation {
            project_id: "project-1".to_string(),
            run_id: id.clone(),
            packet: ObligationPacket {
                blockers: vec!["Still missing.".to_string()],
                ..Default::default()
            },
            provenance_source_id: "call-final".to_string(),
        },
    );
    assert!(matches!(
        complete(&store, &id, BTreeMap::new(), Some(final_with_blocker)).await,
        Err(StatefulRunStoreError::AcceptanceGate(_))
    ));
    assert_eq!(
        store.get_run(&id).await.expect("run").map(|run| run.status),
        Some(StatefulRunStatus::Running)
    );
}

#[tokio::test]
async fn obligations_cannot_be_appended_after_completion() {
    let (_home, _sqlite, store, id) =
        store_with_goal("Answer a lookup.", WorkflowMode::Collaborative).await;
    complete_settled(&store, &id).await;
    let late = store
        .append_obligation(
            "after-terminal".to_string(),
            NewObligation {
                project_id: "project-1".to_string(),
                run_id: id.clone(),
                packet: ObligationPacket {
                    blockers: vec!["Approval is absent.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "call-late".to_string(),
            },
        )
        .await;
    assert!(matches!(
        late,
        Err(StatefulRunStoreError::AcceptanceRunState(
            StatefulRunStatus::Completed
        ))
    ));
}

#[tokio::test]
async fn dependency_vectors_are_bounded_and_unique() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let first = AcceptanceChange::Add {
        origin: AcceptanceOrigin::Derived,
        kind: AcceptanceKind::Manual,
        statement: "First.".to_string(),
        request_span: None,
        terms: CriterionTerms::default(),
    };
    let ledger = store
        .revise_acceptance(&id, 0, vec![first], "call-1")
        .await
        .expect("C1");
    for depends_on in [vec![1; 2], vec![1; 10_000]] {
        let error = store
            .revise_acceptance(
                &id,
                ledger.revision,
                vec![AcceptanceChange::Add {
                    origin: AcceptanceOrigin::Derived,
                    kind: AcceptanceKind::Manual,
                    statement: "Second.".to_string(),
                    request_span: None,
                    terms: CriterionTerms {
                        depends_on,
                        ..CriterionTerms::default()
                    },
                }],
                "call-2",
            )
            .await
            .expect_err("duplicates and oversized vectors refused");
        assert!(matches!(
            error,
            StatefulRunStoreError::Acceptance(AcceptanceError::InvalidDependency)
        ));
        assert_eq!(store.acceptance_ledger(&id).await.expect("ledger"), ledger);
    }
}

#[tokio::test]
async fn a_no_op_refinement_is_not_progress() {
    let (_home, _sqlite, store, id) = store_with_run(WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &id,
            0,
            vec![user_check("All tests must pass.", "pytest -q")],
            "call-1",
        )
        .await
        .expect("criterion");
    assert_eq!(
        store.note_rejected_completion(&id).await.expect("counted"),
        1
    );
    let unchanged = store
        .revise_acceptance(
            &id,
            ledger.revision,
            vec![AcceptanceChange::Refine {
                ordinal: 1,
                statement: None,
                required: None,
                terms: TermsUpdate::default(),
            }],
            "call-noop",
        )
        .await
        .expect("no-op accepted");
    assert_eq!(unchanged.revision, ledger.revision);
    assert_eq!(
        store.note_rejected_completion(&id).await.expect("counted"),
        2
    );
    assert_eq!(
        store.note_rejected_completion(&id).await.expect("counted"),
        3
    );
}

#[tokio::test]
async fn completion_consumes_a_held_verification_lease_and_the_run_stays_running() {
    let (_home, _sqlite, store, id) =
        store_with_goal("Answer a lookup.", WorkflowMode::Autonomous).await;
    let attempt = store
        .begin_verification(&id, "owner-a", 60_000)
        .await
        .expect("attempt starts");
    assert!(matches!(
        store.begin_verification(&id, "owner-b", 60_000).await,
        Err(StatefulRunStoreError::Acceptance(
            AcceptanceError::VerificationLeased(_)
        ))
    ));
    let run = store
        .get_run(&id)
        .await
        .expect("run reads")
        .expect("run exists");
    assert_eq!(run.status, StatefulRunStatus::Running);
    let ledger = store.acceptance_ledger(&id).await.expect("ledger reads");
    let foreign = AcceptanceCommit {
        ledger_revision: ledger.revision,
        workspace_generation: ledger.workspace_generation,
        artifacts: BTreeMap::new(),
        checkers: BTreeMap::new(),
        verification: VerificationClaim {
            owner: "owner-b".to_string(),
            attempt,
        },
        validated_obligation_sequence: None,
    };
    assert!(matches!(
        store
            .complete_run_with_acceptance(&id, completion(run.revision), &foreign, None)
            .await,
        Err(StatefulRunStoreError::AcceptanceChanged)
    ));
    store
        .end_verification(&id, "owner-a", attempt)
        .await
        .expect("refusal ends the attempt");
    assert_eq!(
        complete_settled(&store, &id).await.status,
        StatefulRunStatus::Completed
    );
}

#[tokio::test]
async fn socratic_runs_record_no_evidence_before_execution() {
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
            .expect("skipped"),
        0
    );
    for _ in 0..6 {
        store.bump_workspace_generation(&id).await.expect("bump");
    }
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
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    for _ in 0..6 {
        store
            .record_command_evidence(&id, vec![observed(&ledger, 1, 1)])
            .await
            .expect("recorded");
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stateful_acceptance_evidence")
        .fetch_one(&store.pool)
        .await
        .expect("count reads");
    assert_eq!(rows, 4);
}
