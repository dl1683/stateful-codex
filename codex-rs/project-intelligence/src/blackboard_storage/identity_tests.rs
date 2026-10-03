use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::BlackboardEntryId;
use crate::BlackboardEntryState;
use crate::BlackboardEntryUpdate;
use crate::BlackboardImportance;
use crate::BlackboardKind;
use crate::BlackboardProvenance;
use crate::BlackboardProvenanceKind;
use crate::BlackboardStore;
use crate::BlackboardVerification;
use crate::ConfidenceScore;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::NewBlackboardEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::ProjectRelativePath;
use crate::RootPromotion;

const PROJECT_ID: &str = "project-1";

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

async fn create(store: &BlackboardStore, id: &str, content: &str) {
    store
        .create_entry(
            BlackboardEntryId::parse(id).expect("ID"),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: HierarchyNodeId::parse("node-project").expect("node ID"),
                kind: BlackboardKind::RejectedApproach,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(7_000).expect("confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::Normal,
                root_promotion: RootPromotion::NotPromoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "call-1".to_string(),
                },
            },
        )
        .await
        .expect("entry");
}

/// The backfill reads entries after the covered rowid in batches, records their keys and
/// advances coverage in one step; a batch computed from a stale watermark changes nothing;
/// lookups report each entry's current lifecycle; a new entry makes coverage incomplete.
#[tokio::test]
async fn identities_are_backfilled_and_report_lifecycle() {
    let temp_dir = TempDir::new().expect("tempdir");
    let store = store(&temp_dir).await;
    create(&store, "a", "alpha").await;
    create(&store, "b", "beta").await;
    let (batch, covered, newest) = store
        .identity_backfill_batch(PROJECT_ID, /*limit*/ 1)
        .await
        .expect("batch");
    let first = batch.first().expect("one entry").clone();
    store
        .record_identity_backfill(
            PROJECT_ID,
            covered,
            first.rowid,
            &[("key-alpha".to_string(), first.id.clone())],
        )
        .await
        .expect("record");
    let partial = store
        .identities_complete(PROJECT_ID)
        .await
        .expect("coverage");
    // A batch from the stale watermark is ignored.
    store
        .record_identity_backfill(PROJECT_ID, covered, newest, &[])
        .await
        .expect("stale");
    let (rest, covered, newest) = store
        .identity_backfill_batch(PROJECT_ID, /*limit*/ 10)
        .await
        .expect("batch");
    store
        .record_identity_backfill(
            PROJECT_ID,
            covered,
            newest,
            &rest
                .iter()
                .map(|entry| (format!("key-{}", entry.content), entry.id.clone()))
                .collect::<Vec<_>>(),
        )
        .await
        .expect("record");
    let complete = store
        .identities_complete(PROJECT_ID)
        .await
        .expect("coverage");
    let beta = store
        .get_entry(PROJECT_ID, &BlackboardEntryId::parse("b").expect("ID"))
        .await
        .expect("read")
        .expect("b");
    store
        .update_entry(
            PROJECT_ID,
            &beta.id,
            BlackboardEntryUpdate {
                expected_revision: beta.revision,
                kind: beta.value.kind,
                content: beta.value.content.clone(),
                structured_value: None,
                confidence: beta.value.confidence,
                verification: beta.value.verification,
                importance: beta.value.importance,
                root_promotion: beta.value.root_promotion,
                evidence: Vec::new(),
                premises: Vec::new(),
                state: BlackboardEntryState::Tombstoned,
                superseded_by: None,
                provenance: beta.value.provenance.clone(),
            },
        )
        .await
        .expect("forget");
    let found = store
        .identity_entries(
            PROJECT_ID,
            &["key-alpha".to_string(), "key-beta".to_string()],
        )
        .await
        .expect("lookup");
    create(&store, "c", "gamma").await;
    let after_new = store
        .identities_complete(PROJECT_ID)
        .await
        .expect("coverage");
    assert_eq!(
        (
            first.id.as_str().to_string(),
            partial,
            complete,
            found
                .into_iter()
                .map(|(id, revision, active)| (id.to_string(), revision, active))
                .collect::<Vec<_>>(),
            after_new
        ),
        (
            "a".to_string(),
            false,
            true,
            vec![("a".to_string(), 1, true), ("b".to_string(), 2, false)],
            false
        )
    );
}
