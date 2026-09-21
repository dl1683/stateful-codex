use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server_protocol::BlackboardEntry as ApiEntry;
use codex_app_server_protocol::BlackboardEntryState as ApiEntryState;
use codex_app_server_protocol::BlackboardEvidenceFreshness as ApiEvidenceFreshness;
use codex_app_server_protocol::BlackboardImportance as ApiImportance;
use codex_app_server_protocol::BlackboardKind as ApiKind;
use codex_app_server_protocol::BlackboardProvenance as ApiProvenance;
use codex_app_server_protocol::BlackboardProvenanceKind as ApiProvenanceKind;
use codex_app_server_protocol::BlackboardQueryHit as ApiHit;
use codex_app_server_protocol::BlackboardQueryParams;
use codex_app_server_protocol::BlackboardQueryResponse;
use codex_app_server_protocol::BlackboardRelation as ApiRelation;
use codex_app_server_protocol::BlackboardRelationKind as ApiRelationKind;
use codex_app_server_protocol::BlackboardRootPromotion as ApiRootPromotion;
use codex_app_server_protocol::BlackboardVerification as ApiVerification;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_features::Feature;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardRelationId;
use codex_project_intelligence::BlackboardRelationKind;
use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewBlackboardRelation;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[tokio::test]
async fn blackboard_query_returns_semantic_state_with_relationships_and_provenance() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
    let codex_home_path = AbsolutePathBuf::try_from(codex_home.path().to_path_buf())?;
    let project_root_path = AbsolutePathBuf::try_from(project_root.path().to_path_buf())?;
    let mut server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build_initialized()
        .await?;
    let created: ProjectCreateResponse = server
        .request(|request_id| ClientRequest::ProjectCreate {
            request_id,
            params: ProjectCreateParams {
                name: "Blackboard project".to_string(),
                roots: vec![ProjectRoot {
                    path: project_root_path,
                }],
                metadata: Some(BTreeMap::new()),
                idempotency_key: "blackboard-project".to_string(),
            },
        })
        .await?;

    let sqlite = SqliteConfig::new_for_testing(codex_home_path);
    let hierarchy = HierarchyStore::open(&sqlite).await?;
    let node_id = HierarchyNodeId::parse("project-blackboard")?;
    hierarchy
        .create_node(
            node_id.clone(),
            NewHierarchyNode {
                project_id: created.project.id.clone(),
                parent_id: None,
                kind: NodeKind::Project,
                project_root: None,
                relative_path: ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    let blackboard = BlackboardStore::open(&sqlite).await?;
    let instruction = blackboard
        .create_entry(
            BlackboardEntryId::parse("instruction-1")?,
            NewBlackboardEntry {
                project_id: created.project.id.clone(),
                node_id: node_id.clone(),
                kind: BlackboardKind::Instruction,
                content: "Preserve the explicit project boundary.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000)?,
                verification: BlackboardVerification::UserConfirmed,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "turn-user-1".to_string(),
                },
            },
        )
        .await?;
    let decision = blackboard
        .create_entry(
            BlackboardEntryId::parse("decision-1")?,
            NewBlackboardEntry {
                project_id: created.project.id.clone(),
                node_id,
                kind: BlackboardKind::Decision,
                content: "Threads remain views over shared project intelligence.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(9_500)?,
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-agent-1".to_string(),
                },
            },
        )
        .await?;
    let relation = blackboard
        .create_relation(
            BlackboardRelationId::parse("relation-1")?,
            NewBlackboardRelation {
                project_id: created.project.id.clone(),
                from_entry_id: instruction.id.clone(),
                to_entry_id: decision.id.clone(),
                kind: BlackboardRelationKind::Supports,
                note: Some("The user instruction determines the memory boundary.".to_string()),
                confidence: ConfidenceScore::from_basis_points(9_800)?,
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-agent-1".to_string(),
                },
            },
        )
        .await?;

    let response: BlackboardQueryResponse = server
        .request(|request_id| ClientRequest::BlackboardQuery {
            request_id,
            params: BlackboardQueryParams {
                project_id: created.project.id.clone(),
                text: Some("threads views intelligence".to_string()),
                within_node_id: None,
                limit: Some(10),
            },
        })
        .await?;
    assert_eq!(
        response,
        BlackboardQueryResponse {
            data: vec![ApiHit {
                entry: ApiEntry {
                    id: decision.id.to_string(),
                    project_id: created.project.id.clone(),
                    node_id: decision.value.node_id.to_string(),
                    kind: ApiKind::Decision,
                    content: decision.value.content,
                    structured_value: None,
                    confidence_basis_points: 9_500,
                    verification: ApiVerification::Unverified,
                    importance: ApiImportance::High,
                    root_promotion: ApiRootPromotion::Promoted,
                    evidence: Vec::new(),
                    provenance: ApiProvenance {
                        kind: ApiProvenanceKind::Agent,
                        source_id: "turn-agent-1".to_string(),
                    },
                    state: ApiEntryState::Active,
                    superseded_by: None,
                    revision: decision.revision,
                    created_at: decision.created_at_ms.div_euclid(/*rhs*/ 1000),
                    updated_at: decision.updated_at_ms.div_euclid(/*rhs*/ 1000),
                },
                relations: vec![ApiRelation {
                    id: relation.id.to_string(),
                    project_id: created.project.id,
                    from_entry_id: relation.value.from_entry_id.to_string(),
                    to_entry_id: relation.value.to_entry_id.to_string(),
                    kind: ApiRelationKind::Supports,
                    note: relation.value.note,
                    confidence_basis_points: 9_800,
                    provenance: ApiProvenance {
                        kind: ApiProvenanceKind::Agent,
                        source_id: "turn-agent-1".to_string(),
                    },
                    revision: relation.revision,
                    created_at: relation.created_at_ms.div_euclid(/*rhs*/ 1000),
                    updated_at: relation.updated_at_ms.div_euclid(/*rhs*/ 1000),
                }],
                evidence_freshness: ApiEvidenceFreshness::NotApplicable,
                effective_verification: ApiVerification::Unverified,
            }],
            truncated: false,
        }
    );
    Ok(())
}
