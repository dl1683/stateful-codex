use std::collections::BTreeMap;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::states_criteria;
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

#[test]
fn criteria_words_are_recognized_case_insensitively() {
    for request in [
        "Summarize the parser; it must cover errors.",
        "Make sure the answer cites the module.",
        "Acceptance: the list is complete.",
        "Verify the totals.",
        "The output is REQUIRED to be sorted.",
    ] {
        assert!(states_criteria(request), "{request}");
    }
    for request in [
        "What does the parser module do?",
        "Explain how the cache is invalidated and which files own it.",
        "Which library should I use for retries?",
        "Customs rules (mustard imports) in the docs.",
    ] {
        assert!(!states_criteria(request), "{request}");
    }
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

    let (_home, store, id) = open_run("Explain the parser; it must cover errors.").await;
    let error = complete(&store, &id).await.expect_err("stated criteria");
    assert!(
        error.to_string().contains("covered by no criterion"),
        "{error}"
    );
    assert!(
        !store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .read_only_exemption
    );
}
