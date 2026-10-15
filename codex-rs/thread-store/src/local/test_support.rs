use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::ARCHIVED_SESSIONS_SUBDIR;
use codex_utils_absolute_path::test_support::PathExt;
use uuid::Uuid;

use super::LocalThreadStoreConfig;

pub(super) fn test_config(codex_home: &Path) -> LocalThreadStoreConfig {
    LocalThreadStoreConfig {
        codex_home: codex_home.to_path_buf(),
        sqlite: codex_state::SqliteConfig::new_for_testing(codex_home.abs()),
        default_model_provider_id: "test-provider".to_string(),
    }
}

pub(super) async fn wait_for_backfill(runtime: &codex_state::StateRuntime) {
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        loop {
            if runtime
                .get_backfill_state()
                .await
                .expect("read fixture backfill state")
                .status
                == codex_state::BackfillStatus::Complete
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(/*millis*/ 10)).await;
        }
    })
    .await
    .expect("historical fixture backfill should complete");
}

pub(super) async fn indexed_state_db(
    config: &LocalThreadStoreConfig,
) -> codex_rollout::StateDbHandle {
    let rollout_config = codex_rollout::RolloutConfig {
        codex_home: config.codex_home.clone(),
        sqlite: config.sqlite.clone(),
        cwd: config.codex_home.clone(),
        model_provider_id: config.default_model_provider_id.clone(),
        generate_memories: false,
    };
    let runtime = codex_rollout::state_db::try_init(&rollout_config)
        .await
        .expect("initialize fixture metadata indexing");
    wait_for_backfill(runtime.as_ref()).await;
    runtime
}

pub(super) fn write_session_file(root: &Path, ts: &str, uuid: Uuid) -> std::io::Result<PathBuf> {
    write_session_file_with_history_mode(root, ts, uuid, ThreadHistoryMode::Legacy)
}

pub(super) fn write_session_file_with_history_mode(
    root: &Path,
    ts: &str,
    uuid: Uuid,
    history_mode: ThreadHistoryMode,
) -> std::io::Result<PathBuf> {
    write_session_file_with(
        root,
        root.join("sessions/2025/01/03"),
        ts,
        uuid,
        "Hello from user",
        Some("test-provider"),
        history_mode,
    )
}

pub(super) fn write_archived_session_file(
    root: &Path,
    ts: &str,
    uuid: Uuid,
) -> std::io::Result<PathBuf> {
    write_session_file_with(
        root,
        root.join(ARCHIVED_SESSIONS_SUBDIR),
        ts,
        uuid,
        "Archived user message",
        Some("test-provider"),
        ThreadHistoryMode::Legacy,
    )
}

pub(super) fn write_session_file_with(
    root: &Path,
    day_dir: PathBuf,
    ts: &str,
    uuid: Uuid,
    first_user_message: &str,
    model_provider: Option<&str>,
    history_mode: ThreadHistoryMode,
) -> std::io::Result<PathBuf> {
    write_session_file_with_fork(
        root,
        day_dir,
        ts,
        uuid,
        first_user_message,
        model_provider,
        /*forked_from_id*/ None,
        history_mode,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn write_session_file_with_fork(
    root: &Path,
    day_dir: PathBuf,
    ts: &str,
    uuid: Uuid,
    first_user_message: &str,
    model_provider: Option<&str>,
    forked_from_id: Option<Uuid>,
    history_mode: ThreadHistoryMode,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(&day_dir)?;
    let path = day_dir.join(format!("rollout-{ts}-{uuid}.jsonl"));
    let mut file = fs::File::create(&path)?;
    let mut meta = serde_json::json!({
        "timestamp": ts,
        "type": "session_meta",
        "payload": {
            "session_id": uuid,
            "id": uuid,
            "forked_from_id": forked_from_id,
            "timestamp": ts,
            "cwd": root,
            "originator": "test_originator",
            "cli_version": "test_version",
            "source": "cli",
            "model_provider": model_provider,
            "history_mode": history_mode,
            "git": {
                "commit_hash": "abcdef",
                "branch": "main",
                "repository_url": "https://example.com/repo.git"
            }
        },
    });
    if matches!(history_mode, ThreadHistoryMode::Paginated) {
        meta["ordinal"] = serde_json::json!(0);
    }
    writeln!(file, "{meta}")?;
    if matches!(history_mode, ThreadHistoryMode::Legacy) {
        let user_event = serde_json::json!({
            "timestamp": ts,
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "message": first_user_message,
                "kind": "plain",
            },
        });
        writeln!(file, "{user_event}")?;
    }
    Ok(path)
}
