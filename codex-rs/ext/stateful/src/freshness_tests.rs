use std::sync::Arc;

use codex_extension_api::ExtensionData;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_thread_store::StoredProject;
use codex_thread_store::StoredProjectRoot;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use crate::StatefulExtension;
use crate::root_blackboard::RootBlackboardStatus;
use crate::root_blackboard::render_root_blackboard;
use crate::services::ProjectIntelligenceServices;

#[tokio::test]
async fn changed_promoted_source_is_reaudited_during_the_same_model_turn() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "# Policy\nThreshold: 10\n").expect("write source");
    let sqlite = SqliteConfig::new_for_testing(state_home.path().abs());
    let services = ProjectIntelligenceServices::new(sqlite);
    ProjectIndexer::new(
        services.hierarchy().await.expect("hierarchy").clone(),
        services.context_map().await.expect("context map").clone(),
    )
    .refresh(ProjectIndexRequest {
        project_id: "project-1".to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    let context_hit = services
        .context_map()
        .await
        .expect("context map")
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let project_node = services
        .hierarchy()
        .await
        .expect("hierarchy")
        .project_node("project-1")
        .await
        .expect("project lookup")
        .expect("project node");
    let extension = StatefulExtension {
        projects: Arc::new(InMemoryThreadStore::default()),
        services: Some(services),
        event_sink: None,
        autonomous: None,
    };
    let project = StoredProject {
        id: "project-1".to_string(),
        name: "Policy".to_string(),
        roots: vec![StoredProjectRoot {
            path: project_root.path().display().to_string(),
        }],
        metadata: Default::default(),
        position: 0,
        created_at_ms: 1,
        updated_at_ms: 1,
        recency_at_ms: None,
    };
    let turn_store = ExtensionData::new("turn-2");
    let empty_status = extension.root_blackboard(&project, &turn_store).await;
    let RootBlackboardStatus::Available(_) = &empty_status else {
        panic!("root blackboard should be available");
    };

    extension
        .services
        .as_ref()
        .expect("project intelligence services")
        .blackboard()
        .await
        .expect("blackboard")
        .create_entry(
            BlackboardEntryId::parse("threshold").expect("entry ID"),
            NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: project_node.id,
                kind: BlackboardKind::Number,
                content: "The policy threshold is 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: context_hit.entry.id,
                    source_fingerprint: context_hit.entry.value.source_fingerprint,
                    line_range: Some(EvidenceLineRange { start: 2, end: 2 }),
                }],
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-2".to_string(),
                },
            },
        )
        .await
        .expect("create promoted knowledge");
    let promoted_status = extension.root_blackboard(&project, &turn_store).await;
    let RootBlackboardStatus::Available(_) = &promoted_status else {
        panic!("root blackboard should be available");
    };
    let mut initial_render = String::new();
    render_root_blackboard(&mut initial_render, &promoted_status);
    assert!(initial_render.contains("verification=sourceVerified"));
    assert!(initial_render.contains("evidence=current"));
    assert!(initial_render.contains("policy.md (current)"));
    let initial_audit = turn_store
        .get::<crate::source_freshness::EvidenceAudit>()
        .expect("cached evidence audit");
    let unchanged_status = extension.root_blackboard(&project, &turn_store).await;
    let RootBlackboardStatus::Available(_) = &unchanged_status else {
        panic!("unchanged root blackboard should be available");
    };
    let unchanged_audit = turn_store
        .get::<crate::source_freshness::EvidenceAudit>()
        .expect("reused evidence audit");
    assert!(Arc::ptr_eq(&initial_audit, &unchanged_audit));

    std::fs::write(&source_path, "# Policy\nThreshold: 06\n").expect("replace source");
    let changed_status = extension.root_blackboard(&project, &turn_store).await;
    let RootBlackboardStatus::Available(_) = &changed_status else {
        panic!("root blackboard should be available");
    };
    let persisted = extension
        .services
        .as_ref()
        .expect("project intelligence services")
        .blackboard()
        .await
        .expect("blackboard")
        .root_projection(codex_project_intelligence::RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 1,
        })
        .await
        .expect("reload projection");
    assert_eq!(
        persisted.data[0].evidence_freshness,
        BlackboardEvidenceFreshness::Stale
    );
    let mut rendered = String::new();
    render_root_blackboard(&mut rendered, &changed_status);
    assert!(rendered.contains("verification=stale"));
    assert!(rendered.contains("evidence=stale"));
    assert!(rendered.contains("policy.md (stale)"));
    let persisted_revision = persisted.revision;
    assert!(rendered.contains(&format!(
        "Project intelligence revision: {persisted_revision}"
    )));
}
