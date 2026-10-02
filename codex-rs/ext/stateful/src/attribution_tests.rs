use std::collections::HashMap;

use codex_extension_api::ToolCallOutcome;
use codex_extension_api::ToolPayload;
use codex_project_intelligence::ContextMapEntryId;
use pretty_assertions::assert_eq;

use super::StatefulAttributionCounters;
use super::StatefulAttributionStatus;
use super::StatefulAttributionSummary;
use super::StatefulAttributionTracker;
use crate::source_freshness::EvidenceAudit;
use crate::source_freshness::SourceAuditStatus;

#[test]
fn tracker_reports_bounded_state_reads_writes_and_reuse() {
    let tracker = StatefulAttributionTracker::default();
    tracker.begin("turn-1", "project-1".to_string(), "thread-1".to_string());
    tracker.record_world_state(
        "turn-1",
        3,
        Some(&EvidenceAudit {
            project_id: "project-1".to_string(),
            statuses: HashMap::from([
                (
                    ContextMapEntryId::parse("route-1").expect("route ID"),
                    SourceAuditStatus::Current,
                ),
                (
                    ContextMapEntryId::parse("route-2").expect("route ID"),
                    SourceAuditStatus::Stale,
                ),
            ]),
            cache_key: None,
            hashed_bytes: 42,
            observed_sources: 2,
        }),
        true,
    );
    tracker.record_tool_outcome(
        "turn-1",
        "call-1",
        "blackboard_query",
        ToolCallOutcome::Completed { success: true },
    );
    tracker.record_tool_outcome(
        "turn-1",
        "call-2",
        "blackboard_record_batch",
        ToolCallOutcome::Completed { success: true },
    );
    tracker.record_tool_outcome(
        "turn-1",
        "call-3",
        "evidence_read",
        ToolCallOutcome::Failed {
            handler_executed: true,
        },
    );
    tracker.prepare_material_findings(
        "turn-1",
        "call-4",
        &ToolPayload::Function {
            arguments: serde_json::json!({
                "status": "completed",
                "materialRootFindings": ["E1", "E2"],
                "materialHistoricalFindings": [{"entryId": "entry-1", "revision": 1}]
            })
            .to_string(),
        },
    );
    tracker.record_tool_outcome(
        "turn-1",
        "call-4",
        "stateful_run_update",
        ToolCallOutcome::Completed { success: true },
    );

    let summary = tracker
        .finish("turn-1", StatefulAttributionStatus::Completed)
        .expect("summary");
    assert_eq!(
        summary,
        StatefulAttributionSummary {
            run_id: None,
            project_id: "project-1".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            status: StatefulAttributionStatus::Completed,
            duration_ms: summary.duration_ms,
            counters: StatefulAttributionCounters {
                world_state_samples: 1,
                root_entries_loaded: 3,
                root_evidence_routes_checked: 2,
                root_evidence_routes_current: 1,
                root_evidence_routes_stale: 1,
                root_unique_sources_observed: 2,
                root_source_bytes_hashed: 42,
                stateful_tool_calls: 4,
                failed_stateful_tool_calls: 1,
                knowledge_query_calls: 1,
                blackboard_write_calls: 1,
                run_update_calls: 1,
                material_findings_reused: 3,
                ..Default::default()
            },
        }
    );
    assert_eq!(
        tracker.finish("turn-1", StatefulAttributionStatus::Completed),
        None
    );
}
