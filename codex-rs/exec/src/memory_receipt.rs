//! The end-of-run memory receipt of `codex exec`: the journal watermark is read before the
//! first turn, and the changes after it in this run's thread are counted by the app server
//! when the run ends. No count is kept from notifications.

use codex_app_server_client::InProcessAppServerClient;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemorySummaryParams;
use codex_app_server_protocol::StatefulMemorySummaryResponse;
use codex_app_server_protocol::StatefulRun;
use codex_app_server_protocol::StatefulRunStatus;
use codex_app_server_protocol::StatefulWorkflowMode;

/// Where this run's memory changes start in the project's journal; `None` outside a
/// Stateful project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryWatermark(u64);

async fn summary(
    client: &InProcessAppServerClient,
    request_id: RequestId,
    thread_id: &str,
    since_sequence: Option<u64>,
) -> Option<StatefulMemorySummaryResponse> {
    client
        .request_typed(ClientRequest::StatefulMemorySummary {
            request_id,
            params: StatefulMemorySummaryParams {
                thread_id: thread_id.to_string(),
                since_sequence,
                thread_ids: since_sequence.map(|_| vec![thread_id.to_string()]),
            },
        })
        .await
        .ok()
}

/// Reads the journal position before the run's first turn.
pub(crate) async fn watermark(
    client: &InProcessAppServerClient,
    request_id: RequestId,
    thread_id: &str,
) -> Option<MemoryWatermark> {
    summary(client, request_id, thread_id, /*since_sequence*/ None)
        .await
        .map(|response| MemoryWatermark(response.latest_sequence))
}

/// Counts this run's changes and says what an open run does after the process exits.
pub(crate) async fn receipt(
    client: &InProcessAppServerClient,
    request_id: RequestId,
    thread_id: &str,
    watermark: MemoryWatermark,
    run: Option<&StatefulRun>,
) -> Vec<String> {
    let totals = summary(client, request_id, thread_id, Some(watermark.0))
        .await
        .and_then(|response| response.since);
    lines(totals.as_ref(), run)
}

pub(crate) fn lines(
    totals: Option<&StatefulMemoryChangeTotals>,
    run: Option<&StatefulRun>,
) -> Vec<String> {
    let mut lines = Vec::new();
    match totals {
        Some(totals) => {
            let plural = |count: u32, one: &str, many: &str| {
                if count == 1 {
                    format!("{count} {one}")
                } else {
                    format!("{count} {many}")
                }
            };
            let parts = [
                (totals.saved, "saved"),
                (totals.promoted, "now applied"),
                (totals.corrected, "corrected"),
                (totals.forgotten, "forgotten"),
                (totals.invalidated, "no longer current"),
                (totals.scopes_ended, "investigations ended"),
            ]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, label)| format!("{count} {label}"))
            .chain((totals.commits_remembered > 0).then(|| {
                plural(
                    totals.commits_remembered,
                    "commit remembered from workspace history",
                    "commits remembered from workspace history",
                )
            }))
            .chain((totals.capture_incomplete > 0).then(|| {
                plural(
                    totals.capture_incomplete,
                    "capture could not finish",
                    "captures could not finish",
                )
            }))
            .collect::<Vec<_>>();
            lines.push(if parts.is_empty() {
                "nothing was saved or changed in this thread during the run".to_string()
            } else {
                format!(
                    "changes in this thread during the run: {}",
                    parts.join(", ")
                )
            });
        }
        None => lines.push(
            "this run's changes could not be counted; /memory in the TUI shows what is saved"
                .to_string(),
        ),
    }
    if let Some(run) = run
        && matches!(
            run.status,
            StatefulRunStatus::Pending | StatefulRunStatus::Running | StatefulRunStatus::Paused
        )
        && run.mode != StatefulWorkflowMode::Autonomous
    {
        let mode = match run.mode {
            StatefulWorkflowMode::Autonomous => "Autonomous",
            StatefulWorkflowMode::Collaborative => "Collaborative",
            StatefulWorkflowMode::Socratic => "Socratic",
        };
        lines.push(format!(
            "your {mode} run stays open for next time; nothing works on it after this exits"
        ));
    }
    lines
}

#[cfg(test)]
#[path = "memory_receipt_tests.rs"]
mod tests;
