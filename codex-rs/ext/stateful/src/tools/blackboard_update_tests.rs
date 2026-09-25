use std::path::PathBuf;
use std::sync::Arc;

use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::ContextMapCoverage;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewContextMapEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SourceFingerprint;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;

use super::BlackboardUpdateTool;
use super::MutationArguments;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";
const FACTS: &[u8] = b"Decisive project facts.\n";

fn fingerprint() -> SourceFingerprint {
    SourceFingerprint::parse(format!("sha256:{:x}", Sha256::digest(FACTS)))
        .expect("valid fingerprint")
}

async fn fixture() -> (
    TempDir,
    BlackboardUpdateTool,
    BlackboardEntryId,
    BlackboardEntryId,
    PathBuf,
    String,
) {
    let temp_dir = TempDir::new().expect("tempdir created");
    let project_root = temp_dir.path().join("workspace");
    std::fs::create_dir(&project_root).expect("project root created");
    std::fs::write(project_root.join("facts.md"), FACTS).expect("source written");
    let project_root_text = project_root.display().to_string();
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(temp_dir.path().abs()));
    let project_node_id = HierarchyNodeId::parse("node-project").expect("valid node ID");
    services
        .hierarchy()
        .await
        .expect("hierarchy opens")
        .create_node(
            project_node_id.clone(),
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
        .expect("project node inserts");
    let root_node_id = HierarchyNodeId::parse("node-root").expect("valid node ID");
    services
        .hierarchy()
        .await
        .expect("hierarchy opens")
        .create_node(
            root_node_id.clone(),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: Some(project_node_id.clone()),
                kind: NodeKind::Directory,
                project_root: Some(project_root_text.clone()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: Some(fingerprint()),
            },
        )
        .await
        .expect("root node inserts");
    let file_node_id = HierarchyNodeId::parse("node-file").expect("valid node ID");
    services
        .hierarchy()
        .await
        .expect("hierarchy opens")
        .create_node(
            file_node_id.clone(),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: Some(root_node_id),
                kind: NodeKind::File,
                project_root: Some(project_root_text),
                relative_path: ProjectRelativePath::parse("facts.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(fingerprint()),
            },
        )
        .await
        .expect("file node inserts");
    let context_map_id = ContextMapEntryId::parse("map-facts").expect("valid map ID");
    services
        .context_map()
        .await
        .expect("context map opens")
        .create_entry(
            context_map_id,
            NewContextMapEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: file_node_id,
                source_fingerprint: fingerprint(),
                description: "Decisive project facts.".to_string(),
                routing_terms: vec!["decisive".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("context-map entry inserts");
    let entry_id = BlackboardEntryId::parse("finding-old").expect("valid entry ID");
    let value = NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: project_node_id.clone(),
        kind: BlackboardKind::Claim,
        content: "A potentially reusable conclusion.".to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(7_000).expect("valid confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: RootPromotion::Candidate,
        evidence: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-create".to_string(),
        },
    };
    let blackboard = services.blackboard().await.expect("blackboard opens");
    blackboard
        .create_entry(entry_id.clone(), value.clone())
        .await
        .expect("candidate inserts");
    let successor_id = BlackboardEntryId::parse("finding-new").expect("valid entry ID");
    blackboard
        .create_entry(
            successor_id.clone(),
            NewBlackboardEntry {
                content: "A newer project conclusion.".to_string(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-successor".to_string(),
                },
                ..value
            },
        )
        .await
        .expect("successor inserts");
    let receipt_id = services.read_receipts().issue(
        PROJECT_ID,
        "thread-1",
        "read-call-1",
        BlackboardEvidenceLink {
            context_map_entry_id: ContextMapEntryId::parse("map-facts").expect("valid map ID"),
            source_fingerprint: fingerprint(),
            line_range: Some(EvidenceLineRange { start: 1, end: 1 }),
        },
        FACTS,
    );
    let tool = BlackboardUpdateTool::new(
        PROJECT_ID.to_string(),
        "thread-1".to_string(),
        services,
        Arc::new(InMemoryThreadStore::default()),
        None,
    );
    (
        temp_dir,
        tool,
        entry_id,
        successor_id,
        project_root,
        receipt_id,
    )
}

fn mutation(value: serde_json::Value) -> MutationArguments {
    serde_json::from_value(value).expect("valid mutation")
}

#[tokio::test]
async fn lifecycle_mutations_promote_revise_supersede_and_retire_entries() {
    let (_temp_dir, tool, entry_id, successor_id, project_root, receipt_id) = fixture().await;
    let promoted = tool
        .apply_mutation(
            mutation(json!({
                "action": "setRootPromotion",
                "entryId": entry_id,
                "expectedRevision": 1,
                "rootPromotion": "promoted"
            })),
            "turn-promote",
            std::slice::from_ref(&project_root),
        )
        .await
        .expect("candidate promotes");
    assert_eq!(
        (promoted.revision, promoted.value.root_promotion),
        (2, RootPromotion::Promoted)
    );

    let revised = tool
        .apply_mutation(
            mutation(json!({
                "action": "revise",
                "entryId": entry_id,
                "expectedRevision": 2,
                "content": "The source verifies the reusable conclusion.",
                "confidenceBasisPoints": 9500,
                "verification": "sourceVerified",
                "evidence": [{
                    "readReceiptId": receipt_id
                }]
            })),
            "turn-revise",
            std::slice::from_ref(&project_root),
        )
        .await
        .expect("entry revises");
    let mut expected_value = promoted.value;
    expected_value.content = "The source verifies the reusable conclusion.".to_string();
    expected_value.confidence =
        ConfidenceScore::from_basis_points(9_500).expect("valid confidence");
    expected_value.verification = BlackboardVerification::SourceVerified;
    expected_value.evidence = vec![BlackboardEvidenceLink {
        context_map_entry_id: ContextMapEntryId::parse("map-facts").expect("valid map ID"),
        source_fingerprint: fingerprint(),
        line_range: Some(EvidenceLineRange { start: 1, end: 1 }),
    }];
    expected_value.provenance.source_id = "turn-revise".to_string();
    assert_eq!(revised.value, expected_value);

    let unreceipted_error = tool
        .apply_mutation(
            mutation(json!({
                "action": "revise",
                "entryId": entry_id,
                "expectedRevision": 3,
                "content": "A different source-verified conclusion."
            })),
            "turn-unreceipted",
            std::slice::from_ref(&project_root),
        )
        .await
        .expect_err("source-verified meaning requires a fresh receipt");
    assert!(
        unreceipted_error
            .to_string()
            .contains("fresh evidence_read")
    );

    let stale_error = tool
        .apply_mutation(
            mutation(json!({
                "action": "setRootPromotion",
                "entryId": entry_id,
                "expectedRevision": 2,
                "rootPromotion": "candidate"
            })),
            "turn-stale",
            std::slice::from_ref(&project_root),
        )
        .await
        .expect_err("stale revision fails");
    assert!(stale_error.to_string().contains("expected 2, found 3"));

    let superseded = tool
        .apply_mutation(
            mutation(json!({
                "action": "supersede",
                "entryId": entry_id,
                "expectedRevision": 3,
                "successorEntryId": successor_id
            })),
            "turn-supersede",
            std::slice::from_ref(&project_root),
        )
        .await
        .expect("entry supersedes");
    assert_eq!(
        (superseded.state, superseded.superseded_by),
        (BlackboardEntryState::Superseded, Some(successor_id.clone()))
    );

    let retired = tool
        .apply_mutation(
            mutation(json!({
                "action": "retire",
                "entryId": successor_id,
                "expectedRevision": 1
            })),
            "turn-retire",
            std::slice::from_ref(&project_root),
        )
        .await
        .expect("successor retires");
    assert_eq!(retired.state, BlackboardEntryState::Tombstoned);
}
