use std::collections::BTreeMap;

use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::AcceptanceCommit;
use crate::NewStatefulRun;
use crate::RunBudget;
use crate::StatefulRun;
use crate::StatefulRunId;
use crate::StatefulRunStatus;
use crate::StatefulRunStore;
use crate::StatefulRunStoreError;
use crate::StatefulRunUpdate;
use crate::VerificationClaim;
use crate::WorkflowMode;

const LOOKUP: &str = "What does the parser module do?";

async fn open_store() -> (TempDir, StatefulRunStore) {
    let home = TempDir::new().expect("tempdir");
    let store = StatefulRunStore::open(&SqliteConfig::new_for_testing(home.path().abs()))
        .await
        .expect("store opens");
    (home, store)
}

async fn create(store: &StatefulRunStore, id: &str, thread_id: &str, goal: &str) -> StatefulRunId {
    let id = StatefulRunId::parse(id).expect("run id");
    store
        .create_run(
            id.clone(),
            NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec![thread_id.to_string()],
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
    id
}

async fn open_run(goal: &str) -> (TempDir, StatefulRunStore, StatefulRunId) {
    let (home, store) = open_store().await;
    let id = create(&store, "exempt-run", "thread-1", goal).await;
    (home, store, id)
}

async fn complete(
    store: &StatefulRunStore,
    id: &StatefulRunId,
) -> Result<StatefulRun, StatefulRunStoreError> {
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
async fn a_run_without_any_action_completes_and_records_the_exemption() {
    let (_home, store, id) = open_run(LOOKUP).await;
    let ledger = store.acceptance_ledger(&id).await.expect("ledger");
    assert!(ledger.observed_by_this_process);
    assert!(crate::no_tool_exempt(&ledger));
    assert_eq!(
        complete(&store, &id).await.expect("exempt").status,
        StatefulRunStatus::Completed
    );
    assert!(
        store
            .acceptance_ledger(&id)
            .await
            .expect("ledger")
            .no_tool_exemption
    );
}

#[tokio::test]
async fn any_recorded_action_or_declared_criterion_brings_the_ledger_back() {
    let (_home, store, id) = open_run(LOOKUP).await;
    store.record_host_action(&id).await.expect("action");
    assert!(complete(&store, &id).await.is_err(), "a recorded action");

    let (_home, store, id) = open_run(LOOKUP).await;
    store
        .record_host_action_for_thread("thread-1")
        .await
        .expect("thread action");
    assert!(complete(&store, &id).await.is_err(), "a thread action");

    let (_home, store, id) = open_run(LOOKUP).await;
    store.record_execution(&id).await.expect("execution");
    assert!(complete(&store, &id).await.is_err(), "an observed command");

    let (_home, store, id) = open_run(LOOKUP).await;
    store
        .bump_workspace_generation(&id)
        .await
        .expect("mutation");
    assert!(complete(&store, &id).await.is_err(), "a workspace mutation");

    let (_home, store, id) = open_run(LOOKUP).await;
    store.begin_command(&id, "call-1").await.expect("pending");
    assert!(complete(&store, &id).await.is_err(), "a pending command");

    let (_home, store, id) = open_run(LOOKUP).await;
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
            .no_tool_exemption
    );
}

#[tokio::test]
async fn a_thread_action_counts_only_for_open_runs_bound_to_that_thread() {
    let (_home, store) = open_store().await;
    let bound = create(&store, "bound", "thread-1", LOOKUP).await;
    let other = create(&store, "other", "thread-2", LOOKUP).await;
    store
        .record_host_action_for_thread("thread-1")
        .await
        .expect("thread action");
    let actions = |ledger: crate::AcceptanceLedger| ledger.host_actions;
    assert_eq!(
        (
            actions(store.acceptance_ledger(&bound).await.expect("ledger")),
            actions(store.acceptance_ledger(&other).await.expect("ledger")),
        ),
        (1, 0)
    );
    assert_eq!(
        complete(&store, &other).await.expect("exempt").status,
        StatefulRunStatus::Completed
    );
    // Terminal runs ignore later actions.
    store
        .record_host_action_for_thread("thread-2")
        .await
        .expect("terminal run");
    assert_eq!(
        actions(store.acceptance_ledger(&other).await.expect("ledger")),
        0
    );
}

/// Unknown history is not exempt: a run created by another process (or before observation
/// existed, or under the earlier rule) may have had actions this process never saw.
#[tokio::test]
async fn a_run_this_process_did_not_create_is_not_exempt() {
    for marker in [0_i64, 1, 7] {
        let (_home, store, id) = open_run(LOOKUP).await;
        sqlx::query(
            "UPDATE stateful_acceptance_ledgers SET observation_version = ? WHERE run_id = ?",
        )
        .bind(marker)
        .bind(id.as_str())
        .execute(&store.pool)
        .await
        .expect("foreign observer");
        let ledger = store.acceptance_ledger(&id).await.expect("ledger");
        assert!(!ledger.observed_by_this_process, "marker {marker}");
        assert!(complete(&store, &id).await.is_err(), "marker {marker}");
    }
}
