use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
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
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SourceFingerprint;
use codex_state::SqliteConfig;
use codex_stateful_runtime::ObligationPacket;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::HistoricalFindingReference;
use super::prepare_completion;
use super::sha256_references;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";
const SOURCE_FINGERPRINT: &str =
    "sha256:79647a43c5c02d1c26d56b5ccbac4287f70f7afb62e6697e86941603d65d01ed";

#[test]
fn extracts_only_complete_sha256_references() {
    let first = "a".repeat(64);
    let second = "B".repeat(64);
    let value = format!(
        "valid sha256:{first}; truncated sha256:abcd; oversized sha256:{first}f; valid sha256:{second}"
    );

    assert_eq!(
        sha256_references(&value).collect::<Vec<_>>(),
        vec![format!("sha256:{first}"), format!("sha256:{second}")]
    );
}

#[tokio::test]
async fn renders_an_exact_selected_historical_finding_into_completion() {
    let temp_dir = TempDir::new().expect("tempdir created");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(temp_dir.path().abs()));
    let project_node_id = HierarchyNodeId::parse("node-project").expect("valid node ID");
    let hierarchy = services.hierarchy().await.expect("hierarchy opens");
    hierarchy
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
    let source_fingerprint =
        SourceFingerprint::parse(SOURCE_FINGERPRINT).expect("valid source fingerprint");
    hierarchy
        .create_node(
            root_node_id.clone(),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: Some(project_node_id.clone()),
                kind: NodeKind::Directory,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: Some(source_fingerprint.clone()),
            },
        )
        .await
        .expect("root node inserts");
    let file_node_id = HierarchyNodeId::parse("node-file").expect("valid node ID");
    hierarchy
        .create_node(
            file_node_id.clone(),
            NewHierarchyNode {
                project_id: PROJECT_ID.to_string(),
                parent_id: Some(root_node_id),
                kind: NodeKind::File,
                project_root: Some("C:\\workspace".to_string()),
                relative_path: ProjectRelativePath::parse("facts.md").expect("valid path"),
                region_anchor: None,
                source_fingerprint: Some(source_fingerprint.clone()),
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
            context_map_id.clone(),
            NewContextMapEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: file_node_id,
                source_fingerprint: source_fingerprint.clone(),
                description: "Historical decision evidence.".to_string(),
                routing_terms: vec!["historical".to_string()],
                coverage: ContextMapCoverage::Complete,
            },
        )
        .await
        .expect("context-map entry inserts");

    let evidence = vec![BlackboardEvidenceLink {
        context_map_entry_id: context_map_id,
        source_fingerprint,
        line_range: Some(EvidenceLineRange { start: 4, end: 7 }),
    }];
    let entry_id = BlackboardEntryId::parse("finding-old").expect("valid entry ID");
    let blackboard = services.blackboard().await.expect("blackboard opens");
    blackboard
        .create_entry(
            entry_id.clone(),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: project_node_id.clone(),
                kind: BlackboardKind::Decision,
                content: "The prior threshold was 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("valid confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Candidate,
                evidence: evidence.clone(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-old".to_string(),
                },
            },
        )
        .await
        .expect("historical entry inserts");
    let successor_id = BlackboardEntryId::parse("finding-new").expect("valid entry ID");
    blackboard
        .create_entry(
            successor_id.clone(),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: project_node_id,
                kind: BlackboardKind::Decision,
                content: "The current threshold is 12.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("valid confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Candidate,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-new".to_string(),
                },
            },
        )
        .await
        .expect("successor entry inserts");
    let historical = blackboard
        .update_entry(
            PROJECT_ID,
            &entry_id,
            BlackboardEntryUpdate {
                expected_revision: 1,
                kind: BlackboardKind::Decision,
                content: "The prior threshold was 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("valid confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Candidate,
                evidence,
                state: BlackboardEntryState::Superseded,
                superseded_by: Some(successor_id),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-supersede".to_string(),
                },
            },
        )
        .await
        .expect("entry supersedes");
    let root_revision = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: PROJECT_ID.to_string(),
            max_entries: 256,
        })
        .await
        .expect("root projection loads")
        .revision;
    let packet = ObligationPacket {
        learning: vec![format!(
            "The superseded source is preserved exactly as {SOURCE_FINGERPRINT}."
        )],
        ..Default::default()
    };

    let completion = prepare_completion(
        PROJECT_ID,
        &services,
        &format!("Recovered prior evidence {SOURCE_FINGERPRINT}."),
        &packet,
        root_revision,
        &[],
        &[HistoricalFindingReference {
            entry_id: entry_id.to_string(),
            revision: historical.revision,
        }],
    )
    .await
    .expect("selected historical finding completes");

    assert!(completion.result.contains(SOURCE_FINGERPRINT));
    assert!(completion.result.contains("facts.md:L4-L7"));
    assert_eq!(completion.checklist[0]["category"], "historicalFinding");
}
