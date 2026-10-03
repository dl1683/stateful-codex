use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::CensusEntry;
use super::ChangeCount;
use crate::BlackboardEntryId;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardVerification;
use crate::ChangeOperation;
use crate::ChangeOrigin;
use crate::ChangeRecord;
use crate::ConfidenceScore;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::KnowledgeAuthority;
use crate::KnowledgeCategory;
use crate::KnowledgeContext;
use crate::KnowledgeValidity;
use crate::MAX_CHANGE_PREVIEW_BYTES;
use crate::NewBlackboardEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;
use crate::RootPromotion;

const PROJECT_ID: &str = "project-1";

fn entry(
    kind: BlackboardKind,
    provenance: BlackboardProvenanceKind,
    content: &str,
) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: HierarchyNodeId::parse("node-project").expect("node ID"),
        kind,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::Normal,
        root_promotion: RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: provenance,
            source_id: "source".to_string(),
        },
    }
}

fn change(
    origin: ChangeOrigin,
    category: KnowledgeCategory,
    thread_id: &str,
    preview: &str,
) -> ChangeRecord {
    ChangeRecord {
        operation: ChangeOperation::Saved,
        origin,
        category,
        action_id: None,
        thread_id: Some(thread_id.to_string()),
        turn_id: Some("turn-1".to_string()),
        group_id: None,
        preview: preview.to_string(),
    }
}

async fn store(temp_dir: &TempDir) -> BlackboardStore {
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy opens")
        .create_node(
            HierarchyNodeId::parse("node-project").expect("node ID"),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await
        .expect("project node");
    BlackboardStore::open(&sqlite)
        .await
        .expect("blackboard opens")
}

/// The census classifies active entries without content; totals and pages count only the
/// journal rows after a watermark, optionally only a session's threads; text with `§` and
/// dashes round-trips, and a long preview is cut on a character boundary.
#[tokio::test]
async fn census_totals_and_pages_follow_the_journal() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let rule_text = "Cite § 4.2 \u{2014} never the summary \u{2013} in every answer";
    store
        .create_entry_with_context(
            BlackboardEntryId::parse("rule-1").expect("id"),
            entry(
                BlackboardKind::Instruction,
                BlackboardProvenanceKind::User,
                rule_text,
            ),
            KnowledgeContext::new(KnowledgeCategory::Rule, KnowledgeAuthority::HumanDirect),
            change(
                ChangeOrigin::HostCapture,
                KnowledgeCategory::Rule,
                "thread-1",
                rule_text,
            ),
        )
        .await
        .expect("rule");
    let watermark = store
        .memory_changes_snapshot(PROJECT_ID, 0, None, 1)
        .await
        .expect("head")
        .1
        .expect("one row")
        .sequence;
    let long_preview = format!("{}\u{2014}tail", "a".repeat(MAX_CHANGE_PREVIEW_BYTES - 1));
    store
        .create_entry_with_context(
            BlackboardEntryId::parse("stateful-commit-1").expect("id"),
            entry(
                BlackboardKind::Fact,
                BlackboardProvenanceKind::Maintenance,
                "Commit abc",
            ),
            KnowledgeContext::new(
                KnowledgeCategory::CommitObservation,
                KnowledgeAuthority::HostObserved,
            ),
            change(
                ChangeOrigin::HostObserved,
                KnowledgeCategory::CommitObservation,
                "thread-2",
                &long_preview,
            ),
        )
        .await
        .expect("commit");
    store
        .create_entry(
            BlackboardEntryId::parse("decision-1").expect("id"),
            entry(
                BlackboardKind::Decision,
                BlackboardProvenanceKind::Agent,
                "Use SQLite",
            ),
        )
        .await
        .expect("legacy decision");

    let mut census = store.memory_census(PROJECT_ID).await.expect("census");
    census.sort_by(|left, right| left.id.cmp(&right.id));
    assert!(census.iter().all(|entry| entry.updated_at_ms > 0));
    for entry in &mut census {
        entry.updated_at_ms = 0;
        entry.created_at_ms = 0;
    }
    assert_eq!(
        census,
        vec![
            CensusEntry {
                id: "decision-1".to_string(),
                kind: BlackboardKind::Decision,
                provenance: BlackboardProvenanceKind::Agent,
                root_promotion: RootPromotion::Promoted,
                category: None,
                validity: None,
                verification: BlackboardVerification::Unverified,
                scope_id: None,
                source_sequence: None,
                unit_ordinal: None,
                created_at_ms: 0,
                authority: None,
                updated_at_ms: 0,
            },
            CensusEntry {
                id: "rule-1".to_string(),
                kind: BlackboardKind::Instruction,
                provenance: BlackboardProvenanceKind::User,
                root_promotion: RootPromotion::Promoted,
                category: Some(KnowledgeCategory::Rule),
                validity: Some(KnowledgeValidity::Current),
                verification: BlackboardVerification::Unverified,
                scope_id: None,
                source_sequence: None,
                unit_ordinal: None,
                created_at_ms: 0,
                authority: Some(KnowledgeAuthority::HumanDirect),
                updated_at_ms: 0,
            },
            CensusEntry {
                id: "stateful-commit-1".to_string(),
                kind: BlackboardKind::Fact,
                provenance: BlackboardProvenanceKind::Maintenance,
                root_promotion: RootPromotion::Promoted,
                category: Some(KnowledgeCategory::CommitObservation),
                validity: Some(KnowledgeValidity::Current),
                verification: BlackboardVerification::Unverified,
                scope_id: None,
                source_sequence: None,
                unit_ordinal: None,
                created_at_ms: 0,
                authority: Some(KnowledgeAuthority::HostObserved),
                updated_at_ms: 0,
            },
        ]
    );

    let commit = ChangeCount {
        operation: ChangeOperation::Saved,
        category: KnowledgeCategory::CommitObservation,
        count: 1,
    };
    let rule = ChangeCount {
        operation: ChangeOperation::Saved,
        category: KnowledgeCategory::Rule,
        count: 1,
    };
    assert_eq!(
        store
            .change_totals(PROJECT_ID, 0, None)
            .await
            .expect("totals"),
        vec![commit, rule]
    );
    assert_eq!(
        store
            .change_totals(PROJECT_ID, watermark, None)
            .await
            .expect("totals"),
        vec![commit]
    );
    let session = vec!["thread-1".to_string()];
    assert_eq!(
        store
            .change_totals(PROJECT_ID, 0, Some(&session))
            .await
            .expect("totals"),
        vec![rule]
    );
    assert_eq!(
        store
            .change_totals(PROJECT_ID, 0, Some(&[]))
            .await
            .expect("totals"),
        Vec::new()
    );

    let page = store
        .memory_changes_snapshot(PROJECT_ID, 0, None, 10)
        .await
        .expect("page")
        .0;
    let previews = page
        .iter()
        .map(|change| change.record.preview.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        previews,
        vec![
            rule_text.to_string(),
            format!("{}\u{2026}", "a".repeat(MAX_CHANGE_PREVIEW_BYTES - 3))
        ]
    );
    let page = store
        .memory_changes_snapshot(PROJECT_ID, 0, Some(&session), 10)
        .await
        .expect("page")
        .0;
    assert_eq!(
        page.iter()
            .map(|change| change.sequence)
            .collect::<Vec<_>>(),
        vec![watermark]
    );
    assert_eq!(
        (
            store.sequence_at(PROJECT_ID, 0).await.expect("at"),
            store.sequence_at(PROJECT_ID, i64::MAX).await.expect("at"),
        ),
        (0, watermark + 1)
    );
}

/// A full page can still tell that more rows follow: one row past the page is returned.
#[tokio::test]
async fn a_full_page_reports_that_more_follow() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    for index in 0..=super::MAX_CHANGES_PAGE {
        store
            .record_change(
                PROJECT_ID,
                /*entry*/ None,
                &change(
                    ChangeOrigin::HostCapture,
                    KnowledgeCategory::Rule,
                    "thread-1",
                    &format!("row {index}"),
                ),
            )
            .await
            .expect("row");
    }
    let session = vec!["thread-1".to_string()];
    let page = store
        .memory_changes_snapshot(PROJECT_ID, 0, Some(&session), super::MAX_CHANGES_PAGE + 1)
        .await
        .expect("page")
        .0;
    assert_eq!(
        page.len(),
        usize::try_from(super::MAX_CHANGES_PAGE).expect("fits") + 1
    );
}

/// A succession journals the successor and each replaced entry in one transaction, each under
/// the category its stored context carries (a typed open check replaced by a decision keeps
/// the carried category for the successor; a legacy entry is counted as legacy); a retry of
/// the committed succession journals nothing more.
#[tokio::test]
async fn a_succession_accounts_for_what_it_replaced() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let typed = store
        .create_entry_with_context(
            BlackboardEntryId::parse("old-1").expect("id"),
            entry(
                BlackboardKind::Question,
                BlackboardProvenanceKind::Agent,
                "old-1 \u{2014} open check",
            ),
            KnowledgeContext::new(
                KnowledgeCategory::OpenCheck,
                KnowledgeAuthority::AssistantReported,
            ),
            change(
                ChangeOrigin::ModelTool,
                KnowledgeCategory::OpenCheck,
                "thread-1",
                "old-1 \u{2014} open check",
            ),
        )
        .await
        .expect("typed")
        .0;
    let legacy = store
        .create_entry(
            BlackboardEntryId::parse("old-2").expect("id"),
            entry(
                BlackboardKind::Decision,
                BlackboardProvenanceKind::Agent,
                "old-2 \u{2014} decision",
            ),
        )
        .await
        .expect("legacy");
    let replaced = [typed, legacy]
        .into_iter()
        .map(|created| crate::SupersededEntry {
            id: created.id,
            expected_revision: created.revision,
        })
        .collect::<Vec<_>>();
    let saved = change(
        ChangeOrigin::ModelTool,
        KnowledgeCategory::Decision,
        "thread-1",
        "new decision",
    );
    let ended = ChangeRecord {
        operation: ChangeOperation::Invalidated,
        ..saved.clone()
    };
    let value = entry(
        BlackboardKind::Decision,
        BlackboardProvenanceKind::Agent,
        "new decision",
    );
    for _ in 0..2 {
        store
            .create_successor_accounted(
                BlackboardEntryId::parse("new").expect("id"),
                value.clone(),
                replaced.clone(),
                Some(&saved),
                Some(&ended),
            )
            .await
            .expect("succession");
    }
    let journal = store
        .memory_changes_snapshot(PROJECT_ID, 0, None, 10)
        .await
        .expect("journal")
        .0
        .into_iter()
        .map(|change| {
            (
                change.entry_id.unwrap_or_default(),
                change.record.operation,
                change.record.category,
                change.record.preview,
            )
        })
        .collect::<Vec<_>>();
    let carried = store
        .knowledge_context(PROJECT_ID, &BlackboardEntryId::parse("new").expect("id"))
        .await
        .expect("context")
        .map(|context| context.category);
    assert_eq!(
        (journal, carried),
        (
            vec![
                (
                    "old-1".to_string(),
                    ChangeOperation::Saved,
                    KnowledgeCategory::OpenCheck,
                    "old-1 \u{2014} open check".to_string()
                ),
                (
                    "new".to_string(),
                    ChangeOperation::Saved,
                    KnowledgeCategory::OpenCheck,
                    "new decision".to_string()
                ),
                (
                    "old-1".to_string(),
                    ChangeOperation::Invalidated,
                    KnowledgeCategory::OpenCheck,
                    "old-1 \u{2014} open check".to_string()
                ),
                (
                    "old-2".to_string(),
                    ChangeOperation::Invalidated,
                    KnowledgeCategory::Legacy,
                    "old-2 \u{2014} decision".to_string()
                ),
            ],
            Some(KnowledgeCategory::OpenCheck)
        )
    );
}
