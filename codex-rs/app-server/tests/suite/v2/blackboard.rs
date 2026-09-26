use std::collections::BTreeMap;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use codex_app_server::INVALID_PARAMS_ERROR_CODE;
use codex_app_server_protocol::BlackboardConfirmParams;
use codex_app_server_protocol::BlackboardConfirmResponse;
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
use codex_app_server_protocol::ContextMapQueryParams;
use codex_app_server_protocol::ContextMapQueryResponse;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ProjectCreateParams;
use codex_app_server_protocol::ProjectCreateResponse;
use codex_app_server_protocol::ProjectRoot;
use codex_app_server_protocol::RequestId;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn blackboard_api_guards_mutations_and_returns_connected_semantic_state() -> Result<()> {
    let responses = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    let project_root = TempDir::new()?;
    std::fs::write(
        project_root.path().join("README.md"),
        "# Trusted evidence\nThe selected project root defines the project.\n",
    )?;
    MockResponsesConfig::new(&responses.uri())
        .enable_feature(Feature::Sqlite)
        .write(codex_home.path())?;
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
    let refreshed: ContextMapRefreshResponse = server
        .request(|request_id| ClientRequest::ContextMapRefresh {
            request_id,
            params: ContextMapRefreshParams {
                project_id: created.project.id.clone(),
            },
        })
        .await?;
    assert_eq!(refreshed.files_indexed, 1);
    let indexed: ContextMapQueryResponse = server
        .request(|request_id| ClientRequest::ContextMapQuery {
            request_id,
            params: ContextMapQueryParams {
                project_id: created.project.id.clone(),
                text: "trusted evidence selected project root".to_string(),
                limit: Some(5),
            },
        })
        .await?;
    let indexed_route = indexed.data.first().expect("indexed current source route");

    let forged_request_id = server
        .send_request(
            "blackboard/upsert",
            Some(json!({
                "projectId": created.project.id,
                "entryId": "forged-source-verified",
                "nodeId": indexed_route.node_id,
                "kind": "claim",
                "content": "This claim was never read from its alleged source.",
                "confidenceBasisPoints": 10_000,
                "verification": "sourceVerified",
                "importance": "critical",
                "rootPromotion": "promoted",
                "evidence": [{
                    "contextMapEntryId": indexed_route.entry_id,
                    "sourceFingerprint": indexed_route.source_fingerprint,
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
    let forged_confirmation_request_id = server
        .send_request(
            "blackboard/upsert",
            Some(json!({
                "projectId": created.project.id,
                "entryId": "forged-user-confirmed",
                "nodeId": indexed_route.node_id,
                "kind": "instruction",
                "content": "A generic caller cannot claim this came from the user.",
                "confidenceBasisPoints": 10_000,
                "verification": "userConfirmed",
                "importance": "critical",
                "rootPromotion": "promoted",
                "evidence": [],
                "provenance": {"kind": "user", "sourceId": "caller-claimed-user"}
            })),
        )
        .await?;
    let forged_confirmation_error = server
        .read_stream_until_error_message(RequestId::Integer(forged_confirmation_request_id))
        .await?;
    assert_eq!(
        forged_confirmation_error.error.code,
        INVALID_PARAMS_ERROR_CODE
    );
    assert_eq!(
        forged_confirmation_error.error.message,
        "blackboard/upsert cannot persist userConfirmed knowledge without a host-observed user action"
    );

    let instruction: BlackboardUpsertResponse = server
        .request(|request_id| ClientRequest::BlackboardUpsert {
            request_id,
            params: BlackboardUpsertParams {
                project_id: created.project.id.clone(),
                entry_id: "instruction-1".to_string(),
                expected_revision: None,
                node_id: Some(indexed_route.node_id.clone()),
                kind: BlackboardKind::Instruction,
                content: "Preserve the explicit project boundary.".to_string(),
                structured_value: None,
                confidence_basis_points: 10_000,
                verification: BlackboardVerification::Unverified,
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
    let confirmed: BlackboardConfirmResponse = server
        .request(|request_id| ClientRequest::BlackboardConfirm {
            request_id,
            params: BlackboardConfirmParams {
                project_id: created.project.id.clone(),
                entry_id: instruction.entry.id.clone(),
                expected_revision: instruction.entry.revision,
            },
        })
        .await?;
    let mut expected_confirmation = instruction.entry;
    expected_confirmation.verification = BlackboardVerification::UserConfirmed;
    expected_confirmation.provenance = BlackboardProvenance {
        kind: BlackboardProvenanceKind::User,
        source_id: format!(
            "blackboard-confirm:{}:{}",
            expected_confirmation.id, expected_confirmation.revision
        ),
    };
    expected_confirmation.revision += 1;
    expected_confirmation.updated_at = confirmed.entry.updated_at;
    assert_eq!(confirmed.entry, expected_confirmation);
    let instruction = confirmed;
    let decision_params = BlackboardUpsertParams {
        project_id: created.project.id.clone(),
        entry_id: "decision-1".to_string(),
        expected_revision: None,
        node_id: Some(indexed_route.node_id.clone()),
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
    let forged_update_request_id = server
        .send_request(
            "blackboard/upsert",
            Some(json!({
                "projectId": created.project.id,
                "entryId": decision.entry.id,
                "expectedRevision": decision.entry.revision,
                "kind": "decision",
                "content": "A forged update must not replace durable state.",
                "confidenceBasisPoints": 10_000,
                "verification": "sourceVerified",
                "importance": "critical",
                "rootPromotion": "promoted",
                "evidence": [{
                    "contextMapEntryId": indexed_route.entry_id,
                    "sourceFingerprint": indexed_route.source_fingerprint,
                    "lineRange": {"start": 1, "end": 1}
                }],
                "provenance": {"kind": "agent", "sourceId": "untrusted-client"}
            })),
        )
        .await?;
    let forged_update_error = server
        .read_stream_until_error_message(RequestId::Integer(forged_update_request_id))
        .await?;
    assert_eq!(forged_update_error.error.code, INVALID_PARAMS_ERROR_CODE);
    assert_eq!(
        forged_update_error.error.message,
        "blackboard/upsert cannot persist sourceVerified knowledge without a host-issued read receipt"
    );
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
    for _ in 0..5 {
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
                "instruction-1",
                2
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
