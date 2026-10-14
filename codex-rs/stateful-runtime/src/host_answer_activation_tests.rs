use super::*;
use crate::ObligationOperation;
use crate::OperationReceiptKey;
use pretty_assertions::assert_eq;

/// A5, B3, C1, C2: collection is live, durable, and excludes retired/proposed criteria.
#[tokio::test]
async fn terminal_collection_survives_restart_and_keeps_ledger_evidence() {
    let home = TempDir::new().expect("home");
    let store = open_store(&home).await;
    let run = new_run(&store, "collect", WorkflowMode::Autonomous).await;
    let ledger = store
        .revise_acceptance(
            &run.id,
            /*expected_revision*/ 0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::User,
                kind: AcceptanceKind::Manual,
                statement: "The parser returns a Result.".into(),
                request_span: Some(crate::RequestSpan {
                    start: 0,
                    end: run.value.goal.len(),
                }),
                terms: CriterionTerms {
                    required: true,
                    ..Default::default()
                },
            }],
            "declare",
            /*project_root*/ None,
        )
        .await
        .expect("ledger");
    store
        .apply_obligation_operation(
            &run.id,
            ObligationOperation::Retain {
                issues: vec!["x".into()],
            },
            &OperationReceiptKey::parse("retain").expect("key"),
            "source",
        )
        .await
        .expect("retain");
    store.pool.close().await;
    let store = open_store(&home).await;
    let mut commit = commit_for(&run, "It returns a Result.");
    commit.trailer_issues = vec!["y".into()];
    let AnsweredRunOutcome::Answered(answered, record) = end(&store, &commit).await else {
        panic!("partial");
    };
    assert_eq!(
        (record.outcome(), answered.result),
        (
            HostAnswerOutcome::Partial,
            Some(format!(
                "{PARTIAL_RESULT_HEADER}- y\n- C1 (The parser returns a Result.): not certified by this terminal answer path\n- Open issue (retained): x\n\nIt returns a Result."
            ))
        )
    );
    assert_eq!(
        store.acceptance_ledger(&run.id).await.expect("ledger"),
        ledger
    );
}

/// Admission must remeasure after collecting the criteria, not only the requested outcome.
#[tokio::test]
async fn collected_partial_basis_is_measured_before_any_write() {
    let home = TempDir::new().expect("home");
    let store = open_store(&home).await;
    let run = new_run(&store, "bound", WorkflowMode::Autonomous).await;
    store
        .revise_acceptance(
            &run.id,
            /*expected_revision*/ 0,
            vec![AcceptanceChange::Add {
                origin: AcceptanceOrigin::Derived,
                kind: AcceptanceKind::Manual,
                statement: "Check the result.".into(),
                request_span: None,
                terms: CriterionTerms {
                    required: true,
                    ..Default::default()
                },
            }],
            "declare",
            /*project_root*/ None,
        )
        .await
        .expect("ledger");
    let body = "a".repeat(32_417);
    let commit = commit_for(&run, &body);
    assert_eq!(
        serialized_answer_bytes(TURN, &commit.answer, HostAnswerOutcome::Answered),
        32_768
    );
    assert!(matches!(
        end(&store, &commit).await,
        AnsweredRunOutcome::Refused(_)
    ));
    assert_eq!(
        store.get_run(&run.id).await.expect("run"),
        Some(run.clone())
    );
    assert_eq!(store.host_answer(&run.id).await.expect("answer"), None);
}
