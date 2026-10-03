use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::InvalidationOutcome;
use crate::BlackboardEntryId;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardStoreError;
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
use crate::NewBlackboardEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;
use crate::RootPromotion;

const PROJECT_ID: &str = "project-1";

fn finding(content: &str) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: HierarchyNodeId::parse("node-project").expect("node ID"),
        kind: BlackboardKind::Fact,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::Normal,
        root_promotion: RootPromotion::Promoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "call-1".to_string(),
        },
    }
}

fn invalidated(preview: &str) -> ChangeRecord {
    ChangeRecord {
        operation: ChangeOperation::Invalidated,
        origin: ChangeOrigin::HostObserved,
        category: KnowledgeCategory::Legacy,
        action_id: None,
        thread_id: None,
        turn_id: None,
        group_id: None,
        preview: preview.to_string(),
    }
}

fn legacy(validity: KnowledgeValidity) -> KnowledgeContext {
    KnowledgeContext {
        validity,
        payload: Some(r#"{"invalidation":{"reason":"changed"}}"#.to_string()),
        ..KnowledgeContext::new(KnowledgeCategory::Legacy, KnowledgeAuthority::LegacyUnknown)
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

/// An invalidated entry stays readable: a new revision keeps its content, marks it stale and
/// records the context; each mark journals one row; repeating changes nothing; a stale
/// revision is refused.
#[tokio::test]
async fn invalidated_entries_stay_readable_and_are_journaled_once() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let old = BlackboardEntryId::parse("symbols").expect("ID");
    let doubt = BlackboardEntryId::parse("plan").expect("ID");
    store
        .create_entry(old.clone(), finding("Symbols are y, mth, d."))
        .await
        .expect("created");
    store
        .create_entry(doubt.clone(), finding("Plan includes a history section."))
        .await
        .expect("created");

    let outcome = store
        .invalidate_entry(
            PROJECT_ID,
            &old,
            /*expected_revision*/ 1,
            legacy(KnowledgeValidity::Obsolete),
            &invalidated("Symbols are y, mth, d."),
        )
        .await
        .expect("invalidated");
    let InvalidationOutcome::Invalidated(entry) = outcome else {
        panic!("expected an invalidation, got {outcome:?}");
    };
    assert_eq!(
        (
            entry.revision,
            entry.value.verification,
            entry.value.content.as_str()
        ),
        (2, BlackboardVerification::Stale, "Symbols are y, mth, d.")
    );
    assert_eq!(
        store
            .knowledge_context(PROJECT_ID, &old)
            .await
            .expect("context"),
        Some(legacy(KnowledgeValidity::Obsolete))
    );
    store
        .invalidate_entry(
            PROJECT_ID,
            &doubt,
            /*expected_revision*/ 1,
            legacy(KnowledgeValidity::NeedsCheck),
            &invalidated("Plan includes a history section."),
        )
        .await
        .expect("marked");

    let again = store
        .invalidate_entry(
            PROJECT_ID,
            &old,
            /*expected_revision*/ 2,
            legacy(KnowledgeValidity::NeedsCheck),
            &invalidated("again"),
        )
        .await
        .expect("idempotent");
    assert_eq!(again, InvalidationOutcome::Unchanged);
    let conflict = store
        .invalidate_entry(
            PROJECT_ID,
            &doubt,
            /*expected_revision*/ 1,
            legacy(KnowledgeValidity::Obsolete),
            &invalidated("late"),
        )
        .await;
    assert!(matches!(
        conflict,
        Err(BlackboardStoreError::RevisionConflict {
            expected: 1,
            actual: 2
        })
    ));
    let journal = store
        .memory_changes(
            PROJECT_ID, /*thread_id*/ None, /*after*/ 0, /*limit*/ 10,
        )
        .await
        .expect("journal")
        .into_iter()
        .map(|change| (change.entry_id, change.revision, change.record.operation))
        .collect::<Vec<_>>();
    assert_eq!(
        journal,
        vec![
            (
                Some("symbols".to_string()),
                Some(2),
                ChangeOperation::Invalidated
            ),
            (
                Some("plan".to_string()),
                Some(2),
                ChangeOperation::Invalidated
            ),
        ]
    );
}

/// A needs-check mark is a metadata-only revision: it succeeds even when the entry's premise
/// has since moved on. A successor or a content correction is a new statement and current
/// again; candidates are current entries mentioning the text, filtered before their limit.
#[tokio::test]
async fn validity_follows_the_statement_not_its_lineage() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let id = |value: &str| BlackboardEntryId::parse(value).expect("ID");
    store
        .create_entry(
            id("premise"),
            NewBlackboardEntry {
                verification: BlackboardVerification::UserConfirmed,
                ..finding("Units live in _compact.py.")
            },
        )
        .await
        .expect("premise");
    store
        .create_entry(
            id("derived"),
            NewBlackboardEntry {
                premises: vec![crate::BlackboardPremiseLink {
                    entry_id: id("premise"),
                    revision: 1,
                }],
                ..finding("So years use y everywhere.")
            },
        )
        .await
        .expect("derived");
    store
        .create_entry(id("plan"), finding("Plan keeps y."))
        .await
        .expect("plan");
    store
        .create_entry(id("other"), finding("Unrelated note."))
        .await
        .expect("other");
    let moved = store
        .get_entry(PROJECT_ID, &id("premise"))
        .await
        .expect("read")
        .expect("premise");
    store
        .update_entry(
            PROJECT_ID,
            &id("premise"),
            crate::BlackboardEntryUpdate {
                expected_revision: 1,
                kind: moved.value.kind,
                content: "Units live in units.py.".to_string(),
                structured_value: None,
                confidence: moved.value.confidence,
                verification: moved.value.verification,
                importance: moved.value.importance,
                root_promotion: moved.value.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: crate::BlackboardEntryState::Active,
                superseded_by: None,
                provenance: moved.value.provenance,
            },
        )
        .await
        .expect("premise moved");

    let marked = store
        .invalidate_entry(
            PROJECT_ID,
            &id("derived"),
            /*expected_revision*/ 1,
            legacy(KnowledgeValidity::NeedsCheck),
            &invalidated("premise moved"),
        )
        .await
        .expect("metadata-only revision");
    store
        .invalidate_entry(
            PROJECT_ID,
            &id("plan"),
            /*expected_revision*/ 1,
            legacy(KnowledgeValidity::Obsolete),
            &invalidated("y changed"),
        )
        .await
        .expect("obsolete");
    let successor = store
        .create_successor(
            id("plan-2"),
            finding("Plan uses yr."),
            vec![crate::SupersededEntry {
                id: id("plan"),
                expected_revision: 2,
            }],
        )
        .await
        .expect("successor");
    let validity = |entry: &str| {
        let store = &store;
        let entry = id(entry);
        async move {
            store
                .knowledge_context(PROJECT_ID, &entry)
                .await
                .expect("context")
                .map(|context| context.validity)
        }
    };
    let (candidates, more) = store
        .invalidation_candidates(
            PROJECT_ID,
            super::InvalidationCandidates {
                kinds: &[BlackboardKind::Fact],
                mentioning: "y",
                limit: 1,
            },
        )
        .await
        .expect("candidates");
    assert_eq!(
        (
            matches!(marked, InvalidationOutcome::Invalidated(_)),
            validity("derived").await,
            successor.successor.id.to_string(),
            validity("plan-2").await,
            candidates
                .iter()
                .map(|entry| entry.id.to_string())
                .collect::<Vec<_>>(),
            more,
        ),
        (
            true,
            Some(KnowledgeValidity::NeedsCheck),
            "plan-2".to_string(),
            Some(KnowledgeValidity::Current),
            vec!["plan-2".to_string()],
            true,
        )
    );
}
