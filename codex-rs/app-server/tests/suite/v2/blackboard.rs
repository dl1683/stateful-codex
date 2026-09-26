use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server::INVALID_PARAMS_ERROR_CODE;
use codex_app_server_protocol::BlackboardEntityKind;
use codex_app_server_protocol::BlackboardEntryState;
use codex_app_server_protocol::BlackboardEvidenceFreshness;
use codex_app_server_protocol::BlackboardImportance;
use codex_app_server_protocol::BlackboardKind;
use codex_app_server_protocol::BlackboardProvenance;
use codex_app_server_protocol::BlackboardProvenanceKind;
use codex_app_server_protocol::BlackboardQueryHit;
use codex_app_server_protocol::BlackboardQueryParams;
use codex_app_server_protocol::BlackboardQueryResponse;
use codex_app_server_protocol::BlackboardRelateParams;
use codex_app_server_protocol::BlackboardRelateResponse;
use codex_app_server_protocol::BlackboardRelationKind;
use codex_app_server_protocol::BlackboardRootPromotion;
use codex_app_server_protocol::BlackboardUpdatedNotification;
use codex_app_server_protocol::BlackboardUpsertParams;
use codex_app_server_protocol::BlackboardUpsertResponse;
use codex_app_server_protocol::BlackboardVerification;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_features::Feature;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn blackboard_api_guards_mutations_and_returns_connected_semantic_state() -> Result<()> {
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

    let forged_request_id = server
        .send_request(
            "blackboard/upsert",
            Some(json!({
                "projectId": created.project.id,
                "entryId": "forged-source-verified",
                "nodeId": node_id,
                "kind": "claim",
                "content": "This claim was never read from its alleged source.",
                "confidenceBasisPoints": 10_000,
                "verification": "sourceVerified",
                "importance": "critical",
                "rootPromotion": "promoted",
                "evidence": [{
                    "contextMapEntryId": "copied-current-route",
                    "sourceFingerprint": "sha256:copied-current-fingerprint",
                    "lineRange": {"start": 1, "end": 1}
                }],
                "provenance": {"kind": "agent", "sourceId": "untrusted-client"}
            })),
        )
        .await?;
    let forged_error = server
        .read_stream_until_error_message(RequestId::Integer(forged_request_id))
        .await?;
    assert_eq!(forged_error.error.code, INVALID_PARAMS_ERROR_CODE);
    assert_eq!(
        forged_error.error.message,
        "blackboard/upsert cannot persist sourceVerified knowledge without a host-issued read receipt"
    );

    let instruction: BlackboardUpsertResponse = server
        .request(|request_id| ClientRequest::BlackboardUpsert {
            request_id,
            params: BlackboardUpsertParams {
                project_id: created.project.id.clone(),
                entry_id: "instruction-1".to_string(),
                expected_revision: None,
                node_id: Some(node_id.to_string()),
                kind: BlackboardKind::Instruction,
                content: "Preserve the explicit project boundary.".to_string(),
                structured_value: None,
                confidence_basis_points: 10_000,
                verification: BlackboardVerification::UserConfirmed,
                importance: BlackboardImportance::Critical,
                root_promotion: BlackboardRootPromotion::Promoted,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::User,
                    source_id: "turn-user-1".to_string(),
                },
                state: None,
                superseded_by: None,
            },
        })
        .await?;
    let decision_params = BlackboardUpsertParams {
        project_id: created.project.id.clone(),
        entry_id: "decision-1".to_string(),
        expected_revision: None,
        node_id: Some(node_id.to_string()),
        kind: BlackboardKind::Decision,
        content: "Threads are isolated memory containers.".to_string(),
        structured_value: None,
        confidence_basis_points: 7_500,
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: BlackboardRootPromotion::Candidate,
        evidence: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-agent-1".to_string(),
        },
        state: None,
        superseded_by: None,
    };
    let decision: BlackboardUpsertResponse = server
        .request(|request_id| ClientRequest::BlackboardUpsert {
            request_id,
            params: decision_params.clone(),
        })
        .await?;
    let updated: BlackboardUpsertResponse = server
        .request(|request_id| ClientRequest::BlackboardUpsert {
            request_id,
            params: BlackboardUpsertParams {
                expected_revision: Some(decision.entry.revision),
                node_id: None,
                content: "Threads remain views over shared project intelligence.".to_string(),
                confidence_basis_points: 9_500,
                root_promotion: BlackboardRootPromotion::Promoted,
                state: Some(BlackboardEntryState::Active),
                ..decision_params
            },
        })
        .await?;
    let related: BlackboardRelateResponse = server
        .request(|request_id| ClientRequest::BlackboardRelate {
            request_id,
            params: BlackboardRelateParams {
                project_id: created.project.id.clone(),
                relation_id: "relation-1".to_string(),
                from_entry_id: instruction.entry.id.clone(),
                to_entry_id: updated.entry.id.clone(),
                kind: BlackboardRelationKind::Supports,
                note: Some("The user instruction determines the memory boundary.".to_string()),
                confidence_basis_points: 9_800,
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-agent-1".to_string(),
                },
            },
        })
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
            data: vec![BlackboardQueryHit {
                entry: updated.entry.clone(),
                relations: vec![related.relation.clone()],
                evidence_freshness: BlackboardEvidenceFreshness::NotApplicable,
                effective_verification: BlackboardVerification::Unverified,
            }],
            truncated: false,
        }
    );

    let mut notifications = Vec::new();
    for _ in 0..4 {
        notifications.push(
            server
                .read_notification::<BlackboardUpdatedNotification>("blackboard/updated")
                .await?,
        );
    }
    assert_eq!(
        notifications,
        vec![
            notification(
                &created.project.id,
                BlackboardEntityKind::Entry,
                "instruction-1",
                1
            ),
            notification(
                &created.project.id,
                BlackboardEntityKind::Entry,
                "decision-1",
                1
            ),
            notification(
                &created.project.id,
                BlackboardEntityKind::Entry,
                "decision-1",
                2
            ),
            notification(
                &created.project.id,
                BlackboardEntityKind::Relation,
                "relation-1",
                1
            ),
        ]
    );
    Ok(())
}

fn notification(
    project_id: &str,
    entity_kind: BlackboardEntityKind,
    entity_id: &str,
    revision: u64,
) -> BlackboardUpdatedNotification {
    BlackboardUpdatedNotification {
        project_id: project_id.to_string(),
        entity_kind,
        entity_id: entity_id.to_string(),
        revision,
        cursor: format!("blackboard:{entity_id}:{revision}"),
    }
}
