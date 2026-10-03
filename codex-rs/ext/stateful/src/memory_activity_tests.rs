use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeCount;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::KnowledgeCategory as PiCategory;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::ChangeTotals;
use super::MemoryCounts;
use super::change_totals;
use super::memory_counts;
use crate::memory_add::MemoryAddition;
use crate::memory_add::add_entry;
use crate::memory_controls::ControlOrigin;
use crate::memory_controls::correct_entry;
use crate::memory_controls::forget_entry;
use crate::services::ProjectIntelligenceServices;

/// Counts follow what new work does with each entry; a direct save, correction and forget
/// are each journaled once with the thread they came from, and a retried action adds nothing.
#[tokio::test]
async fn counts_and_journal_follow_direct_controls() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services.project_node_id("project-1").await.expect("node");
    let store = services.blackboard().await.expect("store");
    let add = |addition: MemoryAddition, content: &'static str, action: &'static str| {
        let node_id = node_id.clone();
        async move {
            add_entry(
                store,
                "project-1",
                node_id,
                addition,
                content,
                action,
                "thread-1",
            )
            .await
            .expect("added")
            .0
        }
    };
    let rule = add(
        MemoryAddition::Rule { scope: None },
        "Cite \u{a7} 4.2 \u{2014} never the summary.",
        "a1",
    )
    .await;
    add(
        MemoryAddition::Rule { scope: None },
        "Cite \u{a7} 4.2 \u{2014} never the summary.",
        "a1",
    )
    .await;
    let decision_entry = add(
        MemoryAddition::Decision {
            reason: Some("dashboards read it as minutes".to_string()),
        },
        "Months use mth.",
        "a2",
    )
    .await;
    add(MemoryAddition::Note, "The CI runs on Windows.", "a3").await;
    store
        .create_entry(
            BlackboardEntryId::parse("question-1").expect("id"),
            NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: node_id.clone(),
                kind: BlackboardKind::Question,
                content: "Does the export keep \u{201c}curly\u{201d} quotes?".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(5_000).expect("confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Normal,
                root_promotion: RootPromotion::NotPromoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "agent".to_string(),
                },
            },
        )
        .await
        .expect("question");
    let origin = ControlOrigin {
        thread_id: "thread-2".to_string(),
        action_id: None,
    };
    correct_entry(
        store,
        "project-1",
        &decision_entry.id,
        decision_entry.revision,
        "Months use mo. Reason: shorter",
        &origin,
    )
    .await
    .expect("correct");
    forget_entry(store, "project-1", &rule.id, rule.revision, &origin)
        .await
        .expect("forget");

    assert_eq!(
        memory_counts(store, "project-1").await.expect("counts"),
        MemoryCounts {
            decisions: 1,
            open_checks: 1,
            other: 1,
            ..MemoryCounts::default()
        }
    );
    let totals = |threads: Option<Vec<String>>| async move {
        change_totals(
            &store
                .change_totals("project-1", 0, threads.as_deref())
                .await
                .expect("totals"),
        )
    };
    assert_eq!(
        (
            totals(None).await,
            totals(Some(vec!["thread-1".to_string()])).await,
        ),
        (
            ChangeTotals {
                saved: 3,
                corrected: 1,
                forgotten: 1,
                ..ChangeTotals::default()
            },
            ChangeTotals {
                saved: 3,
                ..ChangeTotals::default()
            },
        )
    );
    let previews = store
        .memory_changes_for_threads("project-1", 0, None, 10)
        .await
        .expect("page")
        .into_iter()
        .map(|change| (change.record.operation, change.record.preview))
        .collect::<Vec<_>>();
    assert_eq!(
        previews[0],
        (
            ChangeOperation::Saved,
            "Cite \u{a7} 4.2 \u{2014} never the summary.".to_string()
        )
    );
}

#[test]
fn commits_are_counted_apart_from_saves() {
    assert_eq!(
        change_totals(&[
            ChangeCount {
                operation: ChangeOperation::Saved,
                category: PiCategory::CommitObservation,
                count: 4,
            },
            ChangeCount {
                operation: ChangeOperation::Saved,
                category: PiCategory::Rule,
                count: 2,
            },
            ChangeCount {
                operation: ChangeOperation::CaptureIncomplete,
                category: PiCategory::Rule,
                count: 1,
            },
        ]),
        ChangeTotals {
            saved: 2,
            commits_remembered: 4,
            capture_incomplete: 1,
            ..ChangeTotals::default()
        }
    );
}
