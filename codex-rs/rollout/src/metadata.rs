use crate::ARCHIVED_SESSIONS_SUBDIR;
use crate::RolloutItem;
use crate::SESSIONS_SUBDIR;
use crate::compression;
use crate::recorder::RolloutRecorder;
use crate::rollout_file_name::RolloutFileName;
use crate::state_db::normalize_cwd_for_state_db;
use chrono::DateTime;
use chrono::NaiveDateTime;
use chrono::Timelike;
use chrono::Utc;
use codex_protocol::RolloutId;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::SessionSource;
use codex_state::BackfillState;
use codex_state::BackfillStats;
use codex_state::BackfillStatus;
use codex_state::DB_ERROR_METRIC;
use codex_state::DB_METRIC_BACKFILL;
use codex_state::DB_METRIC_BACKFILL_DURATION_MS;
use codex_state::ExtractionOutcome;
use codex_state::ThreadMetadataBuilder;
use codex_state::apply_rollout_item;
use std::path::Path;
use std::path::PathBuf;
use tracing::info;
use tracing::warn;

const BACKFILL_BATCH_SIZE: usize = 25;
#[cfg(not(test))]
const BACKFILL_LEASE_SECONDS: i64 = 900;
#[cfg(test)]
const BACKFILL_LEASE_SECONDS: i64 = 1;

pub(crate) fn builder_from_session_meta(
    session_meta: &SessionMetaLine,
    rollout_path: &Path,
) -> Option<ThreadMetadataBuilder> {
    let created_at = parse_timestamp_to_utc(session_meta.meta.timestamp.as_str())?;
    let mut builder = ThreadMetadataBuilder::new(
        session_meta.meta.id,
        rollout_path.to_path_buf(),
        created_at,
        session_meta.meta.source.clone(),
    );
    builder.history_mode = session_meta.meta.history_mode;
    builder.originator =
        (!session_meta.meta.originator.is_empty()).then(|| session_meta.meta.originator.clone());
    builder.model_provider = session_meta.meta.model_provider.clone();
    builder.agent_nickname = session_meta.meta.agent_nickname.clone();
    builder.agent_role = session_meta.meta.agent_role.clone();
    builder.agent_path = session_meta.meta.agent_path.clone();
    builder.cwd = session_meta.meta.cwd.clone();
    builder.cli_version = Some(session_meta.meta.cli_version.clone());
    builder.sandbox_policy = SandboxPolicy::new_read_only_policy();
    builder.approval_mode = AskForApproval::OnRequest;
    if let Some(git) = session_meta.git.as_ref() {
        builder.git_sha = git.commit_hash.as_ref().map(|sha| sha.0.clone());
        builder.git_branch = git.branch.clone();
        builder.git_origin_url = git.repository_url.clone();
    }
    Some(builder)
}

pub fn builder_from_items(
    items: &[RolloutItem],
    rollout_path: &Path,
) -> Option<ThreadMetadataBuilder> {
    if let Some(session_meta) = items.iter().find_map(|item| match item {
        RolloutItem::SessionMeta(meta_line) => Some(meta_line),
        RolloutItem::ResponseItem(_)
        | RolloutItem::InterAgentCommunication(_)
        | RolloutItem::InterAgentCommunicationMetadata { .. }
        | RolloutItem::Compacted(_)
        | RolloutItem::TurnContext(_)
        | RolloutItem::WorldState(_)
        | RolloutItem::RealtimeItem(_)
        | RolloutItem::TokenUsageRecord(_)
        | RolloutItem::RetainedContext(_)
        | RolloutItem::SecurityRiskScore(_)
        | RolloutItem::EventMsg(_) => None,
    }) && let Some(builder) = builder_from_session_meta(session_meta, rollout_path)
    {
        return Some(builder);
    }

    let file_name = rollout_path.file_name()?.to_str()?;
    let file_name = RolloutFileName::parse(file_name)?;
    let created_ts = file_name.timestamp();
    let created_at =
        DateTime::<Utc>::from_timestamp(created_ts.unix_timestamp(), 0)?.with_nanosecond(0)?;
    Some(ThreadMetadataBuilder::new(
        file_name.thread_id(),
        rollout_path.to_path_buf(),
        created_at,
        SessionSource::default(),
    ))
}

/// Returns the rollout ID encoded in a canonical rollout filename.
///
/// Normal rollouts use `rollout-<timestamp>-<thread-id>.jsonl`, where the thread ID and rollout ID
/// are the same. Threads that have been `reverted` use
/// `rollout-<timestamp>-<thread-id>_<rollout-id>.jsonl`, where this returns the ID after `_`.
///
/// This can differ from [`SessionMeta::id`] when `thread/revert` keeps the thread ID stable while
/// switching to a new immutable rollout file.
pub fn rollout_id_from_path(rollout_path: &Path) -> Option<RolloutId> {
    let file_name = rollout_path.file_name()?.to_str()?;
    Some(RolloutFileName::parse(file_name)?.rollout_id())
}

/// Reads the logical fork cutoff without mistaking a revert's history base for its parent.
///
/// Older rollouts lack the explicit cutoff. Their history base is safe to use only when it
/// names the logical parent directly or the current file is the thread's original rollout.
/// An ambiguous legacy revert omits the cutoff rather than reporting another thread's boundary.
pub fn forked_from_ordinal_exclusive(
    meta: &SessionMeta,
    rollout_path: Option<&Path>,
) -> Option<u64> {
    let parent_id = meta.forked_from_id?;
    meta.forked_from_ordinal_exclusive.or_else(|| {
        meta.history_base
            .filter(|base| {
                base.thread_id == parent_id
                    || rollout_path.and_then(rollout_id_from_path) == Some(meta.id)
            })
            .map(|base| base.end_ordinal_exclusive)
    })
}

pub async fn extract_metadata_from_rollout(
    rollout_path: &Path,
    default_provider: &str,
) -> anyhow::Result<ExtractionOutcome> {
    let (items, _thread_id, parse_errors) =
        RolloutRecorder::load_rollout_items(rollout_path).await?;
    if items.is_empty() {
        return Err(anyhow::anyhow!(
            "empty session file: {}",
            rollout_path.display()
        ));
    }
    let builder = builder_from_items(items.as_slice(), rollout_path).ok_or_else(|| {
        anyhow::anyhow!(
            "rollout missing metadata builder: {}",
            rollout_path.display()
        )
    })?;
    let mut metadata = builder.build(default_provider);
    for item in &items {
        apply_rollout_item(&mut metadata, item, default_provider);
    }
    if let Some(updated_at) = file_modified_time_utc(rollout_path).await {
        metadata.updated_at = updated_at;
        metadata.recency_at = updated_at;
    }
    Ok(ExtractionOutcome {
        metadata,
        memory_mode: items.iter().rev().find_map(|item| match item {
            RolloutItem::SessionMeta(meta_line) => meta_line.meta.memory_mode.clone(),
            RolloutItem::ResponseItem(_)
            | RolloutItem::InterAgentCommunication(_)
            | RolloutItem::InterAgentCommunicationMetadata { .. }
            | RolloutItem::Compacted(_)
            | RolloutItem::TurnContext(_)
            | RolloutItem::WorldState(_)
            | RolloutItem::RealtimeItem(_)
            | RolloutItem::TokenUsageRecord(_)
            | RolloutItem::RetainedContext(_)
            | RolloutItem::SecurityRiskScore(_)
            | RolloutItem::EventMsg(_) => None,
        }),
        parse_errors,
    })
}

pub(crate) async fn backfill_sessions(
    runtime: &codex_state::StateRuntime,
    codex_home: &Path,
    default_provider: &str,
) {
    backfill_sessions_with_lease(
        runtime,
        codex_home,
        default_provider,
        BACKFILL_LEASE_SECONDS,
    )
    .await;
}

pub(crate) async fn backfill_sessions_with_lease(
    runtime: &codex_state::StateRuntime,
    codex_home: &Path,
    default_provider: &str,
    backfill_lease_seconds: i64,
) {
    let metric_client = codex_otel::global();
    let timer = metric_client
        .as_ref()
        .and_then(|otel| otel.start_timer(DB_METRIC_BACKFILL_DURATION_MS, &[]).ok());
    let backfill_state = match runtime.get_backfill_state().await {
        Ok(state) => state,
        Err(err) => {
            warn!(
                "failed to read backfill state at {}: {err}",
                codex_home.display()
            );
            BackfillState::default()
        }
    };
    if backfill_state.status == BackfillStatus::Complete {
        return;
    }
    let lease = match runtime.try_claim_backfill(backfill_lease_seconds).await {
        Ok(Some(lease)) => lease,
        Ok(None) => {
            info!(
                "state db backfill already running at {}; skipping duplicate worker",
                codex_home.display()
            );
            return;
        }
        Err(err) => {
            warn!(
                "failed to claim backfill worker at {}: {err}",
                codex_home.display()
            );
            return;
        }
    };

    let sessions_root = codex_home.join(SESSIONS_SUBDIR);
    let archived_root = codex_home.join(ARCHIVED_SESSIONS_SUBDIR);
    let mut rollout_paths: Vec<BackfillRolloutPath> = Vec::new();
    let mut collection_failed = false;
    for (root, archived) in [(sessions_root, false), (archived_root, true)] {
        let exists = match tokio::fs::try_exists(&root).await {
            Ok(exists) => exists,
            Err(err) => {
                warn!("failed to inspect rollout root {}: {err}", root.display());
                collection_failed = true;
                break;
            }
        };
        if !exists {
            continue;
        }
        match collect_rollout_paths(&root).await {
            Ok(paths) => {
                rollout_paths.extend(paths.into_iter().map(|path| BackfillRolloutPath {
                    watermark: backfill_watermark_for_path(codex_home, &path),
                    path,
                    archived,
                }));
            }
            Err(err) => {
                warn!(
                    "failed to collect rollout paths under {}: {err}",
                    root.display()
                );
                collection_failed = true;
                break;
            }
        }
    }
    if collection_failed {
        if let Err(err) = runtime.release_backfill_lease(&lease).await {
            warn!(
                "failed to release incomplete backfill lease at {}: {err}",
                codex_home.display()
            );
        }
        return;
    }
    rollout_paths.sort_by(|a, b| a.watermark.cmp(&b.watermark));
    if let Some(last_watermark) = backfill_state.last_watermark.as_deref() {
        rollout_paths.retain(|entry| entry.watermark.as_str() > last_watermark);
    }

    let mut stats = BackfillStats {
        scanned: 0,
        upserted: 0,
        failed: 0,
    };
    let mut last_watermark = backfill_state.last_watermark.clone();
    let mut lost_lease = false;
    'backfill: for batch in rollout_paths.chunks(BACKFILL_BATCH_SIZE) {
        let mut batch_last_watermark = None;
        for rollout in batch {
            stats.scanned = stats.scanned.saturating_add(1);
            match extract_metadata_from_rollout(&rollout.path, default_provider).await {
                Ok(outcome) => {
                    if outcome.parse_errors > 0
                        && let Some(ref metric_client) = metric_client
                    {
                        let _ = metric_client.counter(
                            DB_ERROR_METRIC,
                            outcome.parse_errors as i64,
                            &[("stage", "backfill_sessions")],
                        );
                    }
                    let mut metadata = outcome.metadata;
                    metadata.cwd = normalize_cwd_for_state_db(&metadata.cwd);
                    let memory_mode = outcome.memory_mode.unwrap_or_else(|| "enabled".to_string());
                    if rollout.archived && metadata.archived_at.is_none() {
                        let fallback_archived_at = metadata.updated_at;
                        metadata.archived_at = file_modified_time_utc(&rollout.path)
                            .await
                            .or(Some(fallback_archived_at));
                    }
                    if let Err(err) = runtime
                        .insert_thread_if_absent_with_memory_mode(&metadata, memory_mode.as_str())
                        .await
                    {
                        stats.failed = stats.failed.saturating_add(1);
                        warn!("failed to seed rollout {}: {err}", rollout.path.display());
                        break 'backfill;
                    } else {
                        stats.upserted = stats.upserted.saturating_add(1);
                        batch_last_watermark = Some(rollout.watermark.clone());
                    }
                }
                Err(err) => {
                    stats.failed = stats.failed.saturating_add(1);
                    warn!(
                        "failed to extract rollout {}: {err}",
                        rollout.path.display()
                    );
                    break 'backfill;
                }
            }
        }

        if let Some(batch_last_watermark) = batch_last_watermark {
            match runtime
                .checkpoint_claimed_backfill(&lease, batch_last_watermark.as_str())
                .await
            {
                Ok(true) => last_watermark = Some(batch_last_watermark),
                Ok(false) => {
                    lost_lease = true;
                    warn!(
                        "state db backfill lease was replaced at {}; stopping stale worker",
                        codex_home.display()
                    );
                    break;
                }
                Err(err) => {
                    stats.failed = stats.failed.saturating_add(1);
                    warn!(
                        "failed to checkpoint backfill at {}: {err}",
                        codex_home.display()
                    );
                    break;
                }
            }
        }
    }
    if stats.failed == 0 && !lost_lease {
        match runtime
            .mark_claimed_backfill_complete(&lease, last_watermark.as_deref())
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                warn!(
                    "state db backfill lease was replaced before completion at {}",
                    codex_home.display()
                );
            }
            Err(err) => {
                stats.failed = stats.failed.saturating_add(1);
                warn!(
                    "failed to mark backfill complete at {}: {err}",
                    codex_home.display()
                );
                if let Err(release_err) = runtime.release_backfill_lease(&lease).await {
                    warn!(
                        "failed to release backfill lease after completion error at {}: {release_err}",
                        codex_home.display()
                    );
                }
            }
        }
    } else if !lost_lease && let Err(err) = runtime.release_backfill_lease(&lease).await {
        warn!(
            "failed to release incomplete backfill lease at {}: {err}",
            codex_home.display()
        );
    }

    info!(
        "state db backfill scanned={}, upserted={}, failed={}",
        stats.scanned, stats.upserted, stats.failed
    );
    if let Some(metric_client) = metric_client {
        let _ = metric_client.counter(
            DB_METRIC_BACKFILL,
            stats.upserted as i64,
            &[("status", "upserted")],
        );
        let _ = metric_client.counter(
            DB_METRIC_BACKFILL,
            stats.failed as i64,
            &[("status", "failed")],
        );
    }
    if let Some(timer) = timer.as_ref() {
        let status = if stats.failed == 0 {
            "success"
        } else if stats.upserted == 0 {
            "failed"
        } else {
            "partial_failure"
        };
        let _ = timer.record(&[("status", status)]);
    }
}

#[derive(Debug, Clone)]
struct BackfillRolloutPath {
    watermark: String,
    path: PathBuf,
    archived: bool,
}

fn backfill_watermark_for_path(codex_home: &Path, path: &Path) -> String {
    path.strip_prefix(codex_home)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

async fn file_modified_time_utc(path: &Path) -> Option<DateTime<Utc>> {
    let modified = compression::file_modified_time(path).await.ok()??;
    DateTime::<Utc>::from_timestamp(modified.unix_timestamp(), modified.nanosecond())
}

fn parse_timestamp_to_utc(ts: &str) -> Option<DateTime<Utc>> {
    const FILENAME_TS_FORMAT: &str = "%Y-%m-%dT%H-%M-%S";
    if let Ok(naive) = NaiveDateTime::parse_from_str(ts, FILENAME_TS_FORMAT) {
        let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        return dt.with_nanosecond(0);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

async fn collect_rollout_paths(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(dir) = stack.pop() {
        let mut read_dir = tokio::fs::read_dir(&dir).await?;
        loop {
            let next_entry = read_dir.next_entry().await?;
            let Some(entry) = next_entry else {
                break;
            };
            let path = entry.path();
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            if let Some(rollout_file) = compression::RolloutFile::from_path(path) {
                paths.push(rollout_file.into_path());
            }
        }
    }
    Ok(paths)
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
