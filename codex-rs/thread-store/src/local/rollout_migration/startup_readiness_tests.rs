use codex_protocol::ThreadId;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_state::BackfillStatus;
use codex_state::RolloutMigrationSkippedRollout;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::LEGACY_TO_PAGINATED_MIGRATION_ID;
use super::LocalThreadStore;
use super::migration_journal_path;
use crate::local::test_support::indexed_state_db;
use crate::local::test_support::test_config;

#[tokio::test]
async fn pending_index_startup_preserves_bookkeeping_then_retries_after_real_backfill() {
    let home = TempDir::new().unwrap();
    let config = test_config(home.path());
    let runtime = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .unwrap();
    let thread_id = ThreadId::new();
    let path = super::tests::write_rollout(home.path(), thread_id, ThreadHistoryMode::Legacy);
    let before_bytes = std::fs::read(&path).unwrap();
    let mut expected_metadata = codex_rollout::read_session_meta_line(&path).await.unwrap();
    let before_backfill = runtime.get_backfill_state().await.unwrap();
    assert_eq!(before_backfill.status, BackfillStatus::Pending);
    let old_skip = RolloutMigrationSkippedRollout {
        rollout_path: "sessions/old-failed-rollout.jsonl".into(),
        rollout_size_bytes: 1,
        rollout_modified_at_ns: 1,
        skip_reason: "failed".into(),
    };
    runtime
        .record_rollout_migration_skip(LEGACY_TO_PAGINATED_MIGRATION_ID, &old_skip)
        .await
        .unwrap();
    let store = LocalThreadStore::new(config.clone(), Some(runtime.clone()));

    store.migrate_rollouts_on_startup().await.unwrap();

    assert_eq!(
        (
            std::fs::read(&path).unwrap(),
            runtime.get_backfill_state().await.unwrap(),
            runtime
                .get_rollout_migration_state(LEGACY_TO_PAGINATED_MIGRATION_ID)
                .await
                .unwrap(),
            runtime
                .list_rollout_migration_skipped_rollouts(LEGACY_TO_PAGINATED_MIGRATION_ID)
                .await
                .unwrap(),
        ),
        (
            before_bytes.clone(),
            before_backfill,
            None,
            vec![old_skip.clone()]
        )
    );

    let lease = runtime
        .try_claim_backfill(/*lease_seconds*/ 60)
        .await
        .unwrap()
        .unwrap();
    let running = runtime.get_backfill_state().await.unwrap();
    assert_eq!(running.status, BackfillStatus::Running);
    let journal = migration_journal_path(home.path(), thread_id);
    std::fs::create_dir_all(journal.parent().unwrap()).unwrap();
    std::fs::write(&journal, "unprocessed recovery marker").unwrap();
    store.migrate_rollouts_on_startup().await.unwrap();
    assert_eq!(
        (
            runtime.get_backfill_state().await.unwrap(),
            runtime
                .get_rollout_migration_state(LEGACY_TO_PAGINATED_MIGRATION_ID)
                .await
                .unwrap(),
            runtime
                .list_rollout_migration_skipped_rollouts(LEGACY_TO_PAGINATED_MIGRATION_ID)
                .await
                .unwrap(),
            std::fs::read_to_string(&journal).unwrap(),
            std::fs::read(&path).unwrap(),
        ),
        (
            running,
            None,
            vec![old_skip.clone()],
            "unprocessed recovery marker".into(),
            before_bytes
        )
    );
    assert!(runtime.release_backfill_lease(&lease).await.unwrap());
    std::fs::remove_file(journal).unwrap();
    let indexer = indexed_state_db(&config).await;
    store.migrate_rollouts_on_startup().await.unwrap();

    expected_metadata.meta.history_mode = ThreadHistoryMode::Paginated;
    assert_eq!(
        serde_json::to_value(codex_rollout::read_session_meta_line(&path).await.unwrap()).unwrap(),
        serde_json::to_value(expected_metadata).unwrap()
    );
    assert_eq!(
        runtime
            .list_rollout_migration_skipped_rollouts(LEGACY_TO_PAGINATED_MIGRATION_ID)
            .await
            .unwrap(),
        vec![old_skip]
    );
    assert!(
        runtime
            .get_rollout_migration_state(LEGACY_TO_PAGINATED_MIGRATION_ID)
            .await
            .unwrap()
            .is_some()
    );
    indexer.close().await;
    runtime.close().await;
}
