//! `statefulMemory/activity`, `statefulMemory/summary` and `statefulMemory/recap`: what project
//! memory holds and what changed in it, counted from the journal of committed changes, and a
//! dated return card assembled from stored memory with no model call.

use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::StatefulMemoryActivityParams;
use codex_app_server_protocol::StatefulMemoryActivityResponse;
use codex_app_server_protocol::StatefulMemoryChange;
use codex_app_server_protocol::StatefulMemoryChangeCategory as ApiCategory;
use codex_app_server_protocol::StatefulMemoryChangeTotals;
use codex_app_server_protocol::StatefulMemoryCounts;
use codex_app_server_protocol::StatefulMemoryOperation;
use codex_app_server_protocol::StatefulMemoryOrigin;
use codex_app_server_protocol::StatefulMemoryRecapParams;
use codex_app_server_protocol::StatefulMemoryRecapResponse;
use codex_app_server_protocol::StatefulMemorySummaryParams;
use codex_app_server_protocol::StatefulMemorySummaryResponse;
use codex_app_server_protocol::StatefulRecapDecision;
use codex_app_server_protocol::StatefulRecapWork;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ChangeOrigin;
use codex_project_intelligence::KnowledgeCategory;
use codex_project_intelligence::MemoryChange;

use super::BlackboardRequestProcessor;
use super::blackboard_error;
use crate::error_code::invalid_params;

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;
/// Most threads one request may restrict a count or page to.
const MAX_THREADS: usize = 64;

impl BlackboardRequestProcessor {
    pub(crate) async fn memory_activity(
        &self,
        params: StatefulMemoryActivityParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let threads = bounded_threads(params.thread_ids)?;
        // A cursor is the last sequence of the previous page in this project's journal.
        let after = match params.cursor.as_deref() {
            None => params.after_sequence.unwrap_or(0),
            Some(cursor) => cursor
                .strip_prefix(&format!("{project_id}:"))
                .and_then(|sequence| sequence.parse::<u64>().ok())
                .ok_or_else(|| invalid_params("cursor is not one this method returned"))?,
        };
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let store = self.store().await?;
        let latest_sequence = store
            .journal_head(&project_id)
            .await
            .map_err(blackboard_error)?
            .map_or(0, |head| head.sequence);
        let mut changes = store
            .memory_changes_for_threads(&project_id, after, threads.as_deref(), limit + 1)
            .await
            .map_err(blackboard_error)?;
        let more = changes.len() > limit as usize;
        changes.truncate(limit as usize);
        let next_cursor = more
            .then(|| {
                changes
                    .last()
                    .map(|last| format!("{project_id}:{}", last.sequence))
            })
            .flatten();
        Ok(Some(
            StatefulMemoryActivityResponse {
                project_id,
                data: changes.into_iter().map(api_change).collect(),
                next_cursor,
                latest_sequence,
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_summary(
        &self,
        params: StatefulMemorySummaryParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let threads = bounded_threads(params.thread_ids)?;
        let store = self.store().await?;
        let head = store
            .journal_head(&project_id)
            .await
            .map_err(blackboard_error)?;
        let counts = codex_stateful_extension::memory_counts(store, &project_id)
            .await
            .map_err(blackboard_error)?;
        let since = match params.since_sequence {
            Some(since) => {
                let counts = store
                    .change_totals(&project_id, since, threads.as_deref())
                    .await
                    .map_err(blackboard_error)?;
                let totals = codex_stateful_extension::change_totals(&counts);
                Some(StatefulMemoryChangeTotals {
                    saved: totals.saved,
                    commits_remembered: totals.commits_remembered,
                    promoted: totals.promoted,
                    corrected: totals.corrected,
                    forgotten: totals.forgotten,
                    invalidated: totals.invalidated,
                    scopes_ended: totals.scopes_ended,
                    capture_incomplete: totals.capture_incomplete,
                })
            }
            None => None,
        };
        Ok(Some(
            StatefulMemorySummaryResponse {
                project_id,
                counts: StatefulMemoryCounts {
                    rules: counts.rules,
                    pending_rules: counts.pending_rules,
                    unverified_rules: counts.unverified_rules,
                    background: counts.background,
                    decisions: counts.decisions,
                    open_checks: counts.open_checks,
                    commits: counts.commits,
                    other: counts.other,
                },
                latest_sequence: head.map_or(0, |head| head.sequence),
                since,
                last_change_at: head.map(|head| head.created_at_ms.div_euclid(1_000)),
            }
            .into(),
        ))
    }

    pub(crate) async fn memory_recap(
        &self,
        params: StatefulMemoryRecapParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project_id = self.thread_project(&params.thread_id).await?;
        let recap = codex_stateful_extension::return_recap(
            self.store().await?,
            self.thread_store.as_ref(),
            &project_id,
            &params.thread_id,
        )
        .await
        .map_err(blackboard_error)?;
        Ok(Some(
            StatefulMemoryRecapResponse {
                project_id,
                as_of: recap.as_of_ms.div_euclid(1_000),
                last_work: recap.last_work.map(|work| StatefulRecapWork {
                    thread_id: work.thread_id,
                    finished_at: work.finished_at_ms.div_euclid(1_000),
                    request: work.request,
                }),
                rules: recap.rules,
                more_rules: recap.more_rules,
                decisions: recap
                    .decisions
                    .into_iter()
                    .map(|decision| StatefulRecapDecision {
                        text: decision.text,
                        reason: decision.reason,
                    })
                    .collect(),
                more_decisions: recap.more_decisions,
                open_checks: recap.open_checks,
                more_open_checks: recap.more_open_checks,
                commits: recap.commits,
                more_commits: recap.more_commits,
                capture_incomplete: recap.capture_incomplete,
            }
            .into(),
        ))
    }
}

fn bounded_threads(
    thread_ids: Option<Vec<String>>,
) -> Result<Option<Vec<String>>, JSONRPCErrorError> {
    match thread_ids {
        Some(ids) if ids.len() > MAX_THREADS => Err(invalid_params(format!(
            "threadIds may name at most {MAX_THREADS} threads"
        ))),
        ids => Ok(ids),
    }
}

fn api_change(change: MemoryChange) -> StatefulMemoryChange {
    let record = change.record;
    StatefulMemoryChange {
        sequence: change.sequence,
        entry_id: change.entry_id,
        revision: change.revision,
        operation: match record.operation {
            ChangeOperation::Saved => StatefulMemoryOperation::Saved,
            ChangeOperation::Promoted => StatefulMemoryOperation::Promoted,
            ChangeOperation::Corrected => StatefulMemoryOperation::Corrected,
            ChangeOperation::Forgotten => StatefulMemoryOperation::Forgotten,
            ChangeOperation::Invalidated => StatefulMemoryOperation::Invalidated,
            ChangeOperation::ScopeEnded => StatefulMemoryOperation::ScopeEnded,
            ChangeOperation::CaptureIncomplete => StatefulMemoryOperation::CaptureIncomplete,
        },
        origin: match record.origin {
            ChangeOrigin::HostCapture => StatefulMemoryOrigin::HostCapture,
            ChangeOrigin::DirectControl => StatefulMemoryOrigin::DirectControl,
            ChangeOrigin::ModelTool => StatefulMemoryOrigin::ModelTool,
            ChangeOrigin::HostObserved => StatefulMemoryOrigin::HostObserved,
        },
        category: match record.category {
            KnowledgeCategory::Rule => ApiCategory::Rule,
            KnowledgeCategory::Background => ApiCategory::Background,
            KnowledgeCategory::AttributedContext => ApiCategory::AttributedContext,
            KnowledgeCategory::Decision => ApiCategory::Decision,
            KnowledgeCategory::BrainstormOption => ApiCategory::BrainstormOption,
            KnowledgeCategory::RuledOut => ApiCategory::RuledOut,
            KnowledgeCategory::OpenCheck => ApiCategory::OpenCheck,
            KnowledgeCategory::Recipe => ApiCategory::Recipe,
            KnowledgeCategory::CodeObservation => ApiCategory::CodeObservation,
            KnowledgeCategory::CommitObservation => ApiCategory::CommitObservation,
            KnowledgeCategory::Note => ApiCategory::Note,
            KnowledgeCategory::Legacy => ApiCategory::Legacy,
        },
        thread_id: record.thread_id,
        turn_id: record.turn_id,
        group_id: record.group_id,
        preview: record.preview,
        created_at: change.created_at_ms.div_euclid(1_000),
    }
}
