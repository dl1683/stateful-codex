//! Publishes the host's journal of a thread's work (see `window_capture`) to project memory once
//! per context window, so the agent never has to spend a model request to remember what it did.
//!
//! A publication is a host-written, unverified, unpromoted maintenance note: activity receipts,
//! not conclusions. Each covers one disjoint suffix of one thread's observations for one
//! project, frozen with its exact content and entry identity when staged, so a retry writes the
//! same entry. Publication happens when compaction or a reset opens a window (closing the
//! previous one); when a new thread of the project starts, for other threads idle long enough
//! that their window is not live; and pending staged publications are retried whenever a
//! thread of the project becomes ready. No routine publication happens per turn, so a keeper
//! resuming the same thread adds nothing until a window closes.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_stateful_runtime::IdleStaging;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;
use codex_stateful_runtime::WindowEvent;
use codex_stateful_runtime::WindowEventKind;
use codex_stateful_runtime::WindowPublication;
use codex_stateful_runtime::WindowPublicationState;
use sha2::Digest;
use sha2::Sha256;

use crate::services::ProjectIntelligenceServices;
use crate::window_capture::command_text;
use crate::window_capture::exit_code;
use crate::window_capture::exit_text;
use crate::window_capture::head;
use crate::window_capture::is_validation_command;

/// Publication content stays under the blackboard's 4,096-byte entry bound.
const MAX_PUBLICATION_BYTES: usize = 3_800;
/// Bytes reserved for the command, test, failure and plan receipts before any path is listed.
const MAX_RECEIPT_LINE_BYTES: usize = 700;
/// Observations read to build one publication (at most 64 members per page, as the in-session
/// spec bounds a publication page; each observation is at most 8 KiB serialized).
const PUBLICATION_PAGE: u32 = 64;
/// Rows handled per recovery page, and the most pages one call drains.
const RECOVERY_PAGE: u32 = 20;
const MAX_RECOVERY_PAGES: usize = 50;
/// A thread that recorded nothing for this long is treated as no longer live when another
/// thread of the project starts; a live window is never published early.
const IDLE_BEFORE_RECOVERY_MS: i64 = 30 * 60 * 1_000;
/// The frozen content of a suffix that has nothing worth publishing.
const NOTHING_TO_PUBLISH: &str = "no receipts";

/// Closes the thread's current window for this project: records the closure durably (so a
/// failed staging is retried when a thread of the project becomes ready), stages its suffix,
/// then delivers every staged publication of the project that is still pending.
pub(crate) async fn publish_window(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    thread_id: &str,
) -> Result<(), StatefulRunStoreError> {
    let store = services.runtime().await?;
    let closed = async {
        let through_seq = store.window_event_watermark(thread_id, project_id).await?;
        store
            .record_window_closure(thread_id, project_id, through_seq)
            .await?;
        stage(store, project_id, thread_id, through_seq).await
    }
    .await;
    deliver_pending(services, store, project_id).await;
    // The caller keeps the window undecided when the closure was not recorded, so the next
    // step retries it.
    closed
}

/// At the start of a new thread, publishes what earlier, idle sessions of the project left open.
pub(crate) async fn publish_open_windows(services: &ProjectIntelligenceServices, project_id: &str) {
    let Ok(store) = services.runtime().await else {
        return;
    };
    let idle_since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|now| i64::try_from(now.as_millis()).ok())
        .map_or(0, |now| now.saturating_sub(IDLE_BEFORE_RECOVERY_MS));
    stage_known_closures(store, project_id).await;
    let mut attempted = Vec::<String>::new();
    for _ in 0..MAX_RECOVERY_PAGES {
        let threads = match store
            .threads_with_unpublished_events(project_id, idle_since, 100)
            .await
        {
            Ok(threads) => threads,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to list unpublished windows");
                break;
            }
        };
        // Threads already attempted (staged, failed, or found active) are not retried here, so
        // a failing thread cannot hold back the others.
        let fresh = threads
            .into_iter()
            .filter(|thread| !attempted.contains(thread))
            .take(RECOVERY_PAGE as usize)
            .collect::<Vec<_>>();
        if fresh.is_empty() {
            break;
        }
        for thread_id in fresh {
            if let Err(error) = stage_idle(store, project_id, &thread_id, idle_since).await {
                tracing::warn!(%thread_id, %error, "failed to stage an idle window's publication");
            }
            attempted.push(thread_id);
        }
    }
    deliver_pending(services, store, project_id).await;
}

/// When a thread of the project becomes ready: stages every thread's closed windows that were
/// never staged (only through each closure, never open work), then delivers publications a
/// crash or a failed write left pending.
pub(crate) async fn recover_pending(services: &ProjectIntelligenceServices, project_id: &str) {
    let Ok(store) = services.runtime().await else {
        return;
    };
    stage_known_closures(store, project_id).await;
    deliver_pending(services, store, project_id).await;
}

async fn stage_known_closures(store: &StatefulRunStore, project_id: &str) {
    let mut attempted = Vec::<String>::new();
    for _ in 0..MAX_RECOVERY_PAGES {
        let closures = match store.unpublished_window_closures(project_id, 100).await {
            Ok(closures) => closures,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to list closed windows");
                return;
            }
        };
        let fresh = closures
            .into_iter()
            .filter(|(thread, _)| !attempted.contains(thread))
            .take(RECOVERY_PAGE as usize)
            .collect::<Vec<_>>();
        if fresh.is_empty() {
            return;
        }
        for (thread_id, through_seq) in fresh {
            if let Err(error) = stage(store, project_id, &thread_id, through_seq).await {
                tracing::warn!(%thread_id, %error, "failed to stage a closed window's publication");
            }
            attempted.push(thread_id);
        }
    }
}

/// The frozen publication of the thread's unpublished suffix through `through_seq`, or `None`
/// when that suffix is empty.
async fn prepare(
    store: &StatefulRunStore,
    project_id: &str,
    thread_id: &str,
    through_seq: u64,
) -> Result<Option<WindowPublication>, StatefulRunStoreError> {
    let from_seq = store
        .window_publication_watermark(thread_id, project_id)
        .await?;
    if through_seq <= from_seq {
        return Ok(None);
    }
    let content = if store
        .has_window_work(thread_id, project_id, from_seq, through_seq)
        .await?
    {
        // The newest page is what a later session can use; older observations of a very long
        // window stay in the journal and the thread's history.
        let events = store
            .window_events_newest_first(
                thread_id,
                project_id,
                from_seq,
                through_seq,
                PUBLICATION_PAGE,
            )
            .await?;
        publication_content(thread_id, from_seq, through_seq, &events)
    } else {
        NOTHING_TO_PUBLISH.to_string()
    };
    Ok(Some(WindowPublication {
        thread_id: thread_id.to_string(),
        from_seq,
        through_seq,
        project_id: project_id.to_string(),
        entry_id: stable_entry_id(project_id, thread_id, from_seq, through_seq),
        content,
        state: WindowPublicationState::Pending,
    }))
}

async fn stage(
    store: &StatefulRunStore,
    project_id: &str,
    thread_id: &str,
    through_seq: u64,
) -> Result<(), StatefulRunStoreError> {
    let Some(publication) = prepare(store, project_id, thread_id, through_seq).await? else {
        return Ok(());
    };
    let staged = store.stage_window_publication(&publication).await?;
    settle_if_empty(store, &staged).await
}

async fn stage_idle(
    store: &StatefulRunStore,
    project_id: &str,
    thread_id: &str,
    idle_since: i64,
) -> Result<(), StatefulRunStoreError> {
    let through_seq = store.window_event_watermark(thread_id, project_id).await?;
    let Some(publication) = prepare(store, project_id, thread_id, through_seq).await? else {
        return Ok(());
    };
    match store
        .stage_idle_window_publication(&publication, idle_since)
        .await?
    {
        IdleStaging::Staged => settle_if_empty(store, &publication).await,
        IdleStaging::BecameActive => Ok(()),
    }
}

/// A window that only exchanged messages (already in the thread's history) has no receipt
/// worth a memory entry: its suffix is closed without writing one.
async fn settle_if_empty(
    store: &StatefulRunStore,
    publication: &WindowPublication,
) -> Result<(), StatefulRunStoreError> {
    if publication.content == NOTHING_TO_PUBLISH {
        store
            .mark_window_publication_published(
                &publication.thread_id,
                &publication.project_id,
                publication.from_seq,
            )
            .await?;
    }
    Ok(())
}

async fn deliver_pending(
    services: &ProjectIntelligenceServices,
    store: &StatefulRunStore,
    project_id: &str,
) {
    let mut after_row = 0;
    for _ in 0..MAX_RECOVERY_PAGES {
        let pending = match store
            .pending_window_publications_after(project_id, after_row, RECOVERY_PAGE)
            .await
        {
            Ok(pending) => pending,
            Err(error) => {
                tracing::warn!(%project_id, %error, "failed to read pending window publications");
                return;
            }
        };
        let Some((last_row, _)) = pending.last() else {
            return;
        };
        // Rows that fail stay pending for a later call; the cursor moves past them.
        after_row = *last_row;
        let (Ok(blackboard), Ok(node_id)) = (
            services.blackboard().await,
            services.project_node_id(project_id).await,
        ) else {
            return;
        };
        for (_, publication) in pending {
            // A message-only suffix interrupted before its acknowledgement writes no entry.
            if publication.content == NOTHING_TO_PUBLISH {
                if let Err(error) = settle_if_empty(store, &publication).await {
                    tracing::warn!(%error, "failed to close a message-only window");
                }
                continue;
            }
            let Ok(id) = BlackboardEntryId::parse(publication.entry_id.clone()) else {
                continue;
            };
            let value = NewBlackboardEntry {
                project_id: project_id.to_string(),
                node_id: node_id.clone(),
                kind: BlackboardKind::Note,
                content: publication.content.clone(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000)
                    .unwrap_or_else(|error| unreachable!("static confidence: {error}")),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Low,
                root_promotion: RootPromotion::NotPromoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Maintenance,
                    source_id: format!(
                        "window-journal:{}:{}-{}",
                        publication.thread_id, publication.from_seq, publication.through_seq
                    ),
                },
            };
            match blackboard.create_entry(id, value).await {
                Ok(_) => {
                    if let Err(error) = store
                        .mark_window_publication_published(
                            &publication.thread_id,
                            project_id,
                            publication.from_seq,
                        )
                        .await
                    {
                        tracing::warn!(%error, "failed to acknowledge a window publication");
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to publish window receipts"),
            }
        }
    }
}

fn stable_entry_id(project_id: &str, thread_id: &str, from_seq: u64, through_seq: u64) -> String {
    let mut hasher = Sha256::new();
    for part in [
        project_id,
        thread_id,
        &from_seq.to_string(),
        &through_seq.to_string(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("stateful-window-receipts-{:x}", hasher.finalize())
}

/// Host receipts for one journal suffix, stated as observations. Command, test, failure and
/// plan receipts are written first with their own bounds; patch paths fill what is left and
/// report what they leave out.
fn publication_content(
    thread_id: &str,
    from_seq: u64,
    through_seq: u64,
    newest_first: &[WindowEvent],
) -> String {
    let mut lines = vec![format!(
        "Host-observed work receipts (activity, not verified conclusions) from thread {thread_id}, events {}-{through_seq}.",
        from_seq + 1
    )];
    if through_seq - from_seq > newest_first.len() as u64 {
        lines.push(format!(
            "Only the newest {} observations are summarized; the thread's history holds the rest.",
            newest_first.len()
        ));
    }
    let commands = newest_first
        .iter()
        .filter(|event| event.event.kind == WindowEventKind::Command)
        .collect::<Vec<_>>();
    if !commands.is_empty() {
        let failed = commands
            .iter()
            .filter(|event| exit_code(event) != Some(0))
            .count();
        lines.push(format!(
            "Commands: {} observed, {failed} without exit code 0.",
            commands.len()
        ));
        if let Some(working) = commands.iter().find(|event| {
            event.event.payload["status"] == "completed"
                && exit_code(event) == Some(0)
                && is_validation_command(command_text(event))
        }) {
            lines.push(receipt_line(format!(
                "Last working test or check command (exit 0, not a test count), in {}, verbatim: `{}`.",
                working.event.payload["cwd"]
                    .as_str()
                    .unwrap_or("an unrecorded directory"),
                command_text(working)
            )));
        }
        if let Some(test) = commands
            .iter()
            .find(|event| is_validation_command(command_text(event)) && exit_code(event) != Some(0))
        {
            lines.push(receipt_line(format!(
                "Last failing test or check command ({}): `{}`.",
                exit_text(test),
                command_text(test)
            )));
        }
        if let Some(failure) = commands.iter().find(|event| exit_code(event) != Some(0)) {
            lines.push(receipt_line(format!(
                "Last command without exit code 0 ({}): `{}`.",
                exit_text(failure),
                command_text(failure)
            )));
        }
    }
    if let Some(plan) = newest_first
        .iter()
        .find(|event| event.event.kind == WindowEventKind::Plan)
    {
        let steps = plan.event.payload["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|step| step["status"] != "completed")
            .take(3)
            .map(|step| {
                format!(
                    "{} ({})",
                    step["step"].as_str().unwrap_or_default(),
                    step["status"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        if !steps.is_empty() {
            lines.push(receipt_line(format!(
                "Agent's last plan (its own words) had open: {}.",
                steps.join("; ")
            )));
        }
    }
    let mut content = lines.join("\n");
    for (outcome, label) in [
        ("applied", "Paths in patches that applied"),
        (
            "failed",
            "Paths in patches that failed (attempted; which files changed was not observed)",
        ),
        ("declined", "Paths in declined patches (not applied)"),
    ] {
        let paths = patch_paths(newest_first, outcome);
        if paths.is_empty() {
            continue;
        }
        let mut line = format!("{label}, newest first:");
        let mut shown = 0;
        for path in &paths {
            let addition = format!(" {path};");
            // Keep room for the omission note.
            if content.len() + 1 + line.len() + addition.len() + 40 > MAX_PUBLICATION_BYTES {
                break;
            }
            line.push_str(&addition);
            shown += 1;
        }
        if shown < paths.len() {
            line.push_str(&format!(" {} more not listed.", paths.len() - shown));
        }
        if content.len() + 1 + line.len() <= MAX_PUBLICATION_BYTES {
            content.push('\n');
            content.push_str(&line);
        }
    }
    head(content.trim(), MAX_PUBLICATION_BYTES)
}

fn receipt_line(line: String) -> String {
    if line.len() <= MAX_RECEIPT_LINE_BYTES {
        return line;
    }
    format!("{} [shortened]", head(&line, MAX_RECEIPT_LINE_BYTES - 12))
}

/// Unique paths of patches with this overall outcome, newest first (paths a patch did not
/// list, because its observation was bounded, are counted where it says so).
fn patch_paths(newest_first: &[WindowEvent], outcome: &str) -> Vec<String> {
    let mut paths = Vec::<String>::new();
    for event in newest_first.iter().filter(|event| {
        event.event.kind == WindowEventKind::Edit && event.event.payload["status"] == outcome
    }) {
        for path in event.event.payload["paths"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let path = path["path"].as_str().unwrap_or_default().to_string();
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
        let more = event.event.payload["morePaths"]
            .as_u64()
            .unwrap_or_default();
        if more > 0 {
            paths.push(format!("({more} more paths in one patch)"));
        }
    }
    paths
}

#[cfg(test)]
#[path = "window_journal_tests.rs"]
mod tests;
