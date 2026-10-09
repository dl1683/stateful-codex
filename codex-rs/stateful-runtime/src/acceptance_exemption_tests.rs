use std::collections::BTreeMap;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::AcceptanceCommit;
use crate::NewStatefulRun;
use crate::RunBudget;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::StatefulRunUpdate;
use crate::VerificationClaim;
use crate::WorkflowMode;

async fn open_run(goal: &str) -> (TempDir, StatefulRunStore, StatefulRunId) {
    let home = TempDir::new().expect("tempdir");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens");
    let id = StatefulRunId::parse("exempt-run").expect("run id");
    store
        .create_run(
            id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: goal.to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 4,
                    max_elapsed_seconds: 3_600,
                },
            },
        )
        .await
        .expect("run created");
    (home, store, id)
}

async fn complete(
    store: &StatefulRunStore,
    id: &StatefulRunId,
) -> Result<crate::StatefulRun, StatefulRunStoreError> {
    let run = store.get_run(id).await?.expect("run");
    let attempt = store.begin_verification(id, "owner", 60_000).await?;
    let ledger = store.acceptance_ledger(id).await?;
    let result = store
        .complete_run_with_acceptance(
            id,
            StatefulRunUpdate {
                expected_revision: run.revision,
                status: StatefulRunStatus::Completed,
                strategy: run.strategy.clone(),
                result: Some("The answer.".to_string()),
            },
            &AcceptanceCommit {
                ledger_revision: ledger.revision,
                workspace_generation: ledger.workspace_generation,
                artifacts: BTreeMap::new(),
                checkers: BTreeMap::new(),
                verification: VerificationClaim {
                    owner: "owner".to_string(),
                    attempt,
                },
                validated_obligation_sequence: None,
            },
            None,
        )
        .await
        .map(|(run, _)| run);
    if result.is_err() {
        store.end_verification(id, "owner", attempt).await?;
    }
    result
}

#[tokio::test]
async fn an_effect_free_answer_completes_without_a_ledger() {
    let (_home, store, id) = open_run("What does the parser module do?").await;
    // Read-only executions do not end the exemption.
    store.record_execution(&id).await.expect("read");
    assert_eq!(
        complete(&store, &id).await.expect("exempt").status,
        StatefulRunStatus::Completed
    );
    assert!(
        store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .read_only_exemption
    );
}

#[tokio::test]
async fn any_observed_effect_or_stated_criterion_brings_the_ledger_back() {
    let (_home, store, id) = open_run("What does the parser module do?").await;
    store.record_side_effect(&id).await.expect("effect");
    assert!(complete(&store, &id).await.is_err(), "a side effect");

    let (_home, store, id) = open_run("What does the parser module do?").await;
    store
        .bump_workspace_generation(&id)
        .await
        .expect("mutation");
    assert!(complete(&store, &id).await.is_err(), "a workspace mutation");

    let (_home, store, id) = open_run("What does the parser module do?").await;
    store.begin_command(&id, "call-1").await.expect("pending");
    assert!(complete(&store, &id).await.is_err(), "a pending command");

    // The request's wording is no authority; a declared criterion is.
    let (_home, store, id) = open_run("Explain the parser; it must cover errors.").await;
    assert!(
        complete(&store, &id).await.is_ok(),
        "prose is not a criterion"
    );
    let (_home, store, id) = open_run("What does the parser module do?").await;
    let request = store.acceptance_request(&id).await.expect("request");
    store
        .revise_acceptance(
            &id,
            0,
            vec![crate::AcceptanceChange::Add {
                origin: crate::AcceptanceOrigin::User,
                kind: crate::AcceptanceKind::Manual,
                statement: "Name every public function.".to_string(),
                request_span: Some(crate::RequestSpan {
                    start: 0,
                    end: request.len(),
                }),
                terms: crate::CriterionTerms {
                    required: true,
                    ..crate::CriterionTerms::default()
                },
            }],
            "call-criterion",
        )
        .await
        .expect("criterion declared");
    assert!(complete(&store, &id).await.is_err(), "a declared criterion");
    assert!(
        !store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .read_only_exemption
    );
}
