use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::InvalidationOutcome;
use super::QualificationState;
use super::SourceDependency;
use crate::BlackboardEntryId;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardPremiseLink;
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
const ROOT: &str = "/repo";

fn entry(node: &str, content: &str, provenance: BlackboardProvenanceKind) -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: HierarchyNodeId::parse(node).expect("node ID"),
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
            kind: provenance,
            source_id: "call-1".to_string(),
        },
    }
}

fn needs_check() -> KnowledgeContext {
    KnowledgeContext {
        validity: KnowledgeValidity::NeedsCheck,
        ..KnowledgeContext::new(KnowledgeCategory::Legacy, KnowledgeAuthority::LegacyUnknown)
    }
}

fn change() -> ChangeRecord {
    ChangeRecord {
        operation: ChangeOperation::Invalidated,
        origin: ChangeOrigin::HostObserved,
        category: KnowledgeCategory::Legacy,
        action_id: None,
        thread_id: None,
        turn_id: None,
        group_id: None,
        preview: "changed".to_string(),
    }
}

async fn store(temp_dir: &TempDir) -> BlackboardStore {
    let sqlite = SqliteConfig::new_for_testing(temp_dir.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite)
        .await
        .expect("hierarchy opens");
    let node = |kind, parent: Option<&str>, root: Option<&str>, path: &str| NewHierarchyNode {
        project_id: PROJECT_ID.to_string(),
        parent_id: parent.map(|parent| HierarchyNodeId::parse(parent).expect("parent")),
        kind,
        project_root: root.map(str::to_string),
        relative_path: if path.is_empty() {
            ProjectRelativePath::root()
        } else {
            ProjectRelativePath::parse(path).expect("path")
        },
        region_anchor: None,
        source_fingerprint: None,
    };
    for (id, value) in [
        ("node-project", node(NodeKind::Project, None, None, "")),
        (
            "node-root",
            node(NodeKind::Directory, Some("node-project"), Some(ROOT), ""),
        ),
        (
            "node-file",
            node(NodeKind::File, Some("node-root"), Some(ROOT), "units.py"),
        ),
    ] {
        hierarchy
            .create_node(HierarchyNodeId::parse(id).expect("node ID"), value)
            .await
            .expect("node");
    }
    BlackboardStore::open(&sqlite)
        .await
        .expect("blackboard opens")
}

/// A needs-check mark keeps the entry readable, makes it stale, journals once, succeeds even
/// when a premise has moved on, refuses a stale revision, and repeating it changes nothing.
#[tokio::test]
async fn marks_keep_the_entry_and_are_guarded_and_idempotent() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let id = |value: &str| BlackboardEntryId::parse(value).expect("ID");
    store
        .create_entry(
            id("premise"),
            NewBlackboardEntry {
                verification: BlackboardVerification::UserConfirmed,
                ..entry(
                    "node-project",
                    "Units live in units.py.",
                    BlackboardProvenanceKind::Agent,
                )
            },
        )
        .await
        .expect("premise");
    store
        .create_entry(
            id("claim"),
            NewBlackboardEntry {
                premises: vec![BlackboardPremiseLink {
                    entry_id: id("premise"),
                    revision: 1,
                }],
                ..entry(
                    "node-project",
                    "Years use y.",
                    BlackboardProvenanceKind::Agent,
                )
            },
        )
        .await
        .expect("claim");
    store
        .invalidate_entry(PROJECT_ID, &id("premise"), 1, needs_check(), &change())
        .await
        .expect("premise marked");

    let marked = store
        .invalidate_entry(PROJECT_ID, &id("claim"), 1, needs_check(), &change())
        .await
        .expect("claim marked despite its premise moving on");
    let again = store
        .invalidate_entry(PROJECT_ID, &id("claim"), 2, needs_check(), &change())
        .await
        .expect("again");
    let late = store
        .invalidate_entry(PROJECT_ID, &id("claim"), 1, needs_check(), &change())
        .await;
    let claim = store
        .get_entry(PROJECT_ID, &id("claim"))
        .await
        .expect("read")
        .expect("kept");
    let journal = store
        .memory_changes(
            PROJECT_ID, /*thread_id*/ None, /*after*/ 0, /*limit*/ 10,
        )
        .await
        .expect("journal")
        .len();
    assert_eq!(
        (
            marked,
            again,
            matches!(late, Err(BlackboardStoreError::RevisionConflict { .. })),
            (
                claim.revision,
                claim.value.content.as_str(),
                claim.value.verification
            ),
            store
                .knowledge_context_at(PROJECT_ID, &id("claim"), 2)
                .await
                .expect("context")
                .map(|context| context.validity),
            journal,
        ),
        (
            InvalidationOutcome::Invalidated,
            InvalidationOutcome::Unchanged,
            true,
            (2, "Years use y.", BlackboardVerification::Stale),
            Some(KnowledgeValidity::NeedsCheck),
            2,
        )
    );
}

/// A job keeps its qualified head, scans agent entries in insertion order with their source
/// dependencies, ignores progress from a replaced generation, and completes only at its
/// watermark.
#[tokio::test]
async fn jobs_resume_by_cursor_and_are_replaced_by_new_targets() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    let id = |value: &str| BlackboardEntryId::parse(value).expect("ID");
    for (entry_id, node, provenance) in [
        ("a", "node-file", BlackboardProvenanceKind::Agent),
        ("user", "node-file", BlackboardProvenanceKind::User),
        ("b", "node-project", BlackboardProvenanceKind::Agent),
    ] {
        store
            .create_entry(id(entry_id), entry(node, entry_id, provenance))
            .await
            .expect("entry");
    }
    let established = store
        .establish_qualified_head(PROJECT_ID, ROOT, "head-1")
        .await
        .expect("established");
    let first = store
        .start_qualification(PROJECT_ID, ROOT, "unused", "head-2", Ok("[]".to_string()))
        .await
        .expect("started");
    let page = store
        .qualification_page(PROJECT_ID, first.cursor, first.watermark, 64)
        .await
        .expect("page");
    let scanned = page
        .iter()
        .map(|item| (item.entry.id.to_string(), item.dependencies.clone()))
        .collect::<Vec<_>>();
    assert!(
        store
            .record_qualification_progress(&first, page[0].sequence)
            .await
            .expect("progress")
    );
    let premature = store
        .complete_qualification(&first)
        .await
        .expect("complete");
    let second = store
        .start_qualification(
            PROJECT_ID,
            ROOT,
            "unused",
            "head-3",
            Err("too many".to_string()),
        )
        .await
        .expect("replaced");
    let stale_progress = store
        .record_qualification_progress(&first, first.watermark)
        .await
        .expect("stale progress");
    let third = store
        .start_qualification(PROJECT_ID, ROOT, "unused", "head-4", Ok("[]".to_string()))
        .await
        .expect("restarted");
    store
        .record_qualification_progress(&third, third.watermark)
        .await
        .expect("progress");
    let completed = store
        .complete_qualification(&third)
        .await
        .expect("complete");
    let jobs = store.qualification_jobs(PROJECT_ID).await.expect("jobs");
    assert_eq!(
        (
            (established.state, established.qualified_head.as_str()),
            (first.state, first.cursor, first.watermark, first.generation),
            scanned,
            premature,
            (
                second.state,
                second.reason.as_deref(),
                second.qualified_head.as_str()
            ),
            stale_progress,
            completed,
            (
                jobs[0].state,
                jobs[0].qualified_head.as_str(),
                jobs[0].generation
            ),
        ),
        (
            (QualificationState::Complete, "head-1"),
            (QualificationState::Scanning, 0, 3, 2),
            vec![
                (
                    "a".to_string(),
                    vec![SourceDependency {
                        project_root: ROOT.to_string(),
                        relative_path: "units.py".to_string(),
                        directory: false,
                    }],
                ),
                ("b".to_string(), Vec::new()),
            ],
            false,
            (QualificationState::Blocked, Some("too many"), "head-1"),
            false,
            true,
            (QualificationState::Complete, "head-4", 4),
        )
    );
}
