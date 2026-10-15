use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::ThreadItem;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use uuid::Uuid;

use super::super::LocalThreadStore;
use super::super::test_support::test_config;
use super::super::test_support::write_session_file_with;
use super::ThreadSearchItem;
use super::cursor_from_thread_search_item;
use crate::SearchThreadsParams;
use crate::SortDirection;
use crate::ThreadSortKey;
use crate::ThreadStore;

#[tokio::test]
async fn pending_index_search_is_unavailable_then_retries_after_real_backfill() {
    let home = TempDir::new().unwrap();
    let config = test_config(home.path());
    let runtime = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .unwrap();
    let uuid = Uuid::new_v4();
    let thread_id = ThreadId::from_string(&uuid.to_string()).unwrap();
    super::super::test_support::write_session_file(home.path(), "2025-01-03T12-00-00", uuid)
        .unwrap();
    let store = LocalThreadStore::new(config.clone(), Some(runtime.clone()));
    let query = SearchThreadsParams {
        page_size: 10,
        cursor: None,
        sort_key: ThreadSortKey::CreatedAt,
        sort_direction: SortDirection::Desc,
        allowed_sources: vec![SessionSource::Cli],
        archived: false,
        search_term: "Hello".into(),
    };

    let error = store
        .search_threads(query.clone())
        .await
        .expect_err("pending index must not look empty");
    assert_eq!(
        error.to_string(),
        "thread-store internal error: state DB is not ready for thread search: historical metadata backfill is incomplete"
    );

    let indexer = super::super::test_support::indexed_state_db(&config).await;
    let page = store.search_threads(query).await.unwrap();
    assert_eq!(
        (
            page.items
                .iter()
                .map(|item| (item.thread.thread_id, item.snippet.as_str()))
                .collect::<Vec<_>>(),
            page.next_cursor,
        ),
        (vec![(thread_id, "Hello from user")], None)
    );
    indexer.close().await;
    runtime.close().await;
}

#[tokio::test]
async fn pending_index_search_validation_read_errors_and_sqlite_less_fallback() {
    let home = TempDir::new().unwrap();
    let config = test_config(home.path());
    let runtime = codex_state::StateRuntime::init(
        config.sqlite.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .unwrap();
    let uuid = Uuid::new_v4();
    let thread_id = ThreadId::from_string(&uuid.to_string()).unwrap();
    super::super::test_support::write_session_file(home.path(), "2025-01-03T12-00-00", uuid)
        .unwrap();
    let store = LocalThreadStore::new(config.clone(), Some(runtime.clone()));
    let mut query = SearchThreadsParams {
        page_size: 10,
        cursor: None,
        sort_key: ThreadSortKey::CreatedAt,
        sort_direction: SortDirection::Desc,
        allowed_sources: vec![SessionSource::Cli],
        archived: false,
        search_term: String::new(),
    };
    assert_eq!(
        store
            .search_threads(query.clone())
            .await
            .unwrap_err()
            .to_string(),
        "invalid thread-store request: thread/search requires search_term"
    );
    query.search_term = "Hello".into();
    query.cursor = Some("not-a-cursor".into());
    assert_eq!(
        store
            .search_threads(query.clone())
            .await
            .unwrap_err()
            .to_string(),
        "invalid thread-store request: invalid cursor: not-a-cursor"
    );
    query.cursor = None;
    runtime.close().await;
    assert!(matches!(
        store.search_threads(query.clone()).await,
        Err(crate::ThreadStoreError::Internal { message })
            if message.starts_with("failed to read state DB readiness for thread search:")
    ));

    let filesystem_store = LocalThreadStore::new(config, /*state_db*/ None);
    let page = filesystem_store.search_threads(query).await.unwrap();
    assert_eq!(
        (
            page.items
                .iter()
                .map(|item| (item.thread.thread_id, item.snippet.as_str()))
                .collect::<Vec<_>>(),
            page.next_cursor,
        ),
        (vec![(thread_id, "Hello from user")], None)
    );
}

#[test]
fn recency_cursor_includes_thread_id_tie_breaker() {
    let thread_id = ThreadId::from_string("00000000-0000-0000-0000-000000000123")
        .expect("thread ID should parse");
    let item = ThreadSearchItem {
        item: ThreadItem {
            thread_id: Some(thread_id),
            recency_at: Some("2026-01-27T12:34:56Z".to_string()),
            ..Default::default()
        },
        snippet: String::new(),
    };

    let cursor = cursor_from_thread_search_item(&item, ThreadSortKey::RecencyAt)
        .expect("cursor should build");

    assert_eq!(
        serde_json::to_string(&cursor).expect("cursor should serialize"),
        format!("\"2026-01-27T12:34:56Z|{thread_id}\"")
    );
}

#[tokio::test]
async fn search_matches_selected_rollout_across_path_spellings_and_compression() {
    for compressed in [false, true] {
        let home = TempDir::new().expect("temp dir");
        let config = test_config(home.path());
        let state_db = crate::local::test_support::indexed_state_db(&config).await;
        let uuid = Uuid::new_v4();
        let thread_id = ThreadId::from_string(&uuid.to_string()).expect("thread ID should parse");
        let selected_path = write_session_file_with(
            home.path(),
            home.path().join("sessions/2025/01/03"),
            "2025-01-03T12-00-00",
            uuid,
            "needle in selected rollout",
            Some("test-provider"),
            ThreadHistoryMode::Legacy,
        )
        .expect("write selected rollout");
        // Reverting a thread leaves its previous rollout on disk with the same thread ID.
        write_session_file_with(
            home.path(),
            home.path().join("sessions/2025/01/03"),
            "2025-01-03T11-00-00",
            uuid,
            "obsolete needle in previous rollout",
            Some("test-provider"),
            ThreadHistoryMode::Legacy,
        )
        .expect("write unselected rollout");

        let home_paths = [home.path().to_path_buf()];
        #[cfg(windows)]
        let home_paths = {
            let verbatim = std::fs::canonicalize(&home_paths[0]).expect("canonicalize home");
            let ordinary =
                codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(&verbatim)
                    .expect("normalize home")
                    .into_path_buf();
            [ordinary, verbatim]
        };
        let relative_path = selected_path
            .strip_prefix(home.path())
            .expect("relative rollout path");
        let selected_paths = home_paths.each_ref().map(|home| home.join(relative_path));
        #[cfg(windows)]
        assert_ne!(selected_paths[0], selected_paths[1]);

        if compressed {
            let contents = std::fs::read(&selected_path).expect("read selected rollout");
            let contents = zstd::stream::encode_all(contents.as_slice(), /*level*/ 3)
                .expect("compress selected rollout");
            std::fs::write(selected_path.with_extension("jsonl.zst"), contents)
                .expect("write compressed rollout");
            std::fs::remove_file(&selected_path).expect("remove plain rollout");
        }
        let selected_paths = selected_paths
            .into_iter()
            .flat_map(|path| {
                let compressed_path = compressed.then(|| path.with_extension("jsonl.zst"));
                std::iter::once(path).chain(compressed_path)
            })
            .collect::<Vec<_>>();

        for codex_home in home_paths {
            let store = LocalThreadStore::new(
                super::super::LocalThreadStoreConfig {
                    codex_home,
                    ..config.clone()
                },
                Some(state_db.clone()),
            );
            for selected_path in &selected_paths {
                let mut metadata = codex_state::ThreadMetadataBuilder::new(
                    thread_id,
                    selected_path.clone(),
                    Utc::now(),
                    SessionSource::Cli,
                )
                .build("test-provider");
                metadata.first_user_message = Some("needle in selected rollout".to_string());
                state_db
                    .upsert_thread(&metadata)
                    .await
                    .expect("select rollout in database");

                for (search_term, expected) in [
                    ("needle", vec![(thread_id, "needle in selected rollout")]),
                    ("obsolete", Vec::new()),
                ] {
                    let page = store
                        .search_threads(SearchThreadsParams {
                            page_size: 10,
                            cursor: None,
                            sort_key: ThreadSortKey::CreatedAt,
                            sort_direction: SortDirection::Desc,
                            allowed_sources: vec![SessionSource::Cli],
                            archived: false,
                            search_term: search_term.to_string(),
                        })
                        .await
                        .expect("search selected rollout");
                    assert_eq!(
                        (
                            page.items
                                .iter()
                                .map(|item| (item.thread.thread_id, item.snippet.as_str()))
                                .collect::<Vec<_>>(),
                            page.next_cursor,
                        ),
                        (expected, None),
                        "compressed={compressed}, selected_path={selected_path:?}",
                    );
                }
            }
        }
    }
}
