use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryScope;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapQuery;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::HierarchySourceUpdate;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NodeLifecycle;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SourceFingerprint;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::AuditedEvidenceFreshness;
use super::MAX_AUDITED_SOURCE_BYTES;
use super::MAX_TOTAL_AUDITED_BYTES;
use super::SourceAuditStatus;
use super::SourceCheck;
use super::SourceCheckCache;
use super::audited_blackboard_freshness;
use super::audited_context_freshness;
use super::audited_verification;
use super::observe_evidence;
use super::reconcile_source_state;
use crate::services::ProjectIntelligenceServices;

#[tokio::test]
async fn file_and_region_routes_share_one_physical_source_check() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_text = (1..=70)
        .map(|line| {
            if line == 70 {
                "decisive_route_fact".to_string()
            } else {
                format!("line {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(project_root.path().join("policy.md"), &source_text).expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
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
    let context_map = services.context_map().await.expect("context map");
    let file_hit = context_map
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("file route lookup")
        .into_iter()
        .next()
        .expect("file route");
    let region_hit = context_map
        .query(ContextMapQuery {
            project_id: "project-1".to_string(),
            text: "decisive_route_fact".to_string(),
            max_results: 1,
        })
        .await
        .expect("region route lookup")
        .into_iter()
        .next()
        .expect("region route");
    let roots = [project_root.path().to_path_buf()];
    let mut checks = SourceCheckCache::default();

    let file_check = checks
        .check(
            &roots,
            &file_hit,
            MAX_TOTAL_AUDITED_BYTES,
            MAX_AUDITED_SOURCE_BYTES,
        )
        .await;
    let region_check = checks
        .check(
            &roots,
            &region_hit,
            MAX_TOTAL_AUDITED_BYTES,
            MAX_AUDITED_SOURCE_BYTES,
        )
        .await;

    assert_eq!(file_check, region_check);
    assert_eq!(checks.checks.len(), 1);
    assert_eq!(
        checks.hashed_bytes,
        u64::try_from(source_text.len()).expect("source length fits u64")
    );
}

#[tokio::test]
async fn stale_audit_observation_cannot_regress_a_concurrent_refresh() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    std::fs::write(project_root.path().join("policy.md"), "threshold=10\n").expect("write source");
    let sqlite = SqliteConfig::new_for_testing(state_home.path().abs());
    let hierarchy = HierarchyStore::open(&sqlite).await.expect("hierarchy");
    let context_map = ContextMapStore::open(&sqlite).await.expect("context map");
    ProjectIndexer::new(hierarchy.clone(), context_map.clone())
        .refresh(ProjectIndexRequest {
            project_id: "project-1".to_string(),
            roots: vec![project_root.path().to_path_buf()],
        })
        .await
        .expect("index source");
    let hit = context_map
        .file_hits_for_path(
            "project-1",
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let observed_node = hierarchy
        .get_node("project-1", &hit.entry.value.node_id)
        .await
        .expect("node lookup")
        .expect("source node");
    let digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let refreshed_fingerprint =
        SourceFingerprint::parse(format!("sha256:{digest}")).expect("fingerprint");
    let refreshed_node = hierarchy
        .update_source_state(
            "project-1",
            &observed_node.id,
            HierarchySourceUpdate {
                expected_revision: observed_node.revision,
                lifecycle: NodeLifecycle::Active,
                source_fingerprint: Some(refreshed_fingerprint.clone()),
            },
        )
        .await
        .expect("simulate concurrent refresh");

    let audited_bytes = 13;
    let status = reconcile_source_state(
        "project-1",
        &hierarchy,
        &hit,
        &observed_node,
        SourceCheck::Fingerprint(refreshed_fingerprint, audited_bytes),
    )
    .await;

    assert_eq!(status, SourceAuditStatus::Stale);
    let current_node = hierarchy
        .get_node("project-1", &hit.entry.value.node_id)
        .await
        .expect("node lookup")
        .expect("source node");
    assert_eq!(current_node, refreshed_node);
}

#[tokio::test]
async fn deeper_knowledge_observation_detects_changed_bytes_without_mutating_state() {
    let state_home = TempDir::new().expect("temporary state home");
    let project_root = TempDir::new().expect("temporary project root");
    let source_path = project_root.path().join("policy.md");
    std::fs::write(&source_path, "threshold=10\n").expect("write source");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
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
    services
        .blackboard()
        .await
        .expect("blackboard")
        .create_entry(
            BlackboardEntryId::parse("deeper-threshold").expect("entry ID"),
            NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: context_hit.entry.value.node_id.clone(),
                kind: BlackboardKind::Number,
                content: "The policy threshold is 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::NotPromoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: context_hit.entry.id.clone(),
                    source_fingerprint: context_hit.entry.value.source_fingerprint,
                    line_range: None,
                }],
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: "turn-1".to_string(),
                },
            },
        )
        .await
        .expect("create deeper knowledge");
    let blackboard = services.blackboard().await.expect("blackboard");
    let query = BlackboardQuery {
        project_id: "project-1".to_string(),
        text: Some("threshold".to_string()),
        within_node: None,
        root_promotion: None,
        entry_scope: BlackboardEntryScope::Active,
        max_results: 1,
    };
    let result = blackboard.query(query).await.expect("query knowledge");
    let hierarchy = services.hierarchy().await.expect("hierarchy");
    let node_before = hierarchy
        .get_node("project-1", &context_hit.entry.value.node_id)
        .await
        .expect("node lookup")
        .expect("source node");
    let revision_before = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 1,
        })
        .await
        .expect("root projection")
        .revision;
    std::fs::write(&source_path, "threshold=60\n").expect("change source");

    let audit = observe_evidence(
        &services,
        "project-1",
        &[project_root.path().to_path_buf()],
        [context_hit.entry.id.clone()],
    )
    .await;
    let freshness = audited_blackboard_freshness(&result.data[0], Some(&audit));

    assert_eq!(freshness, AuditedEvidenceFreshness::Stale);
    assert_eq!(
        audited_context_freshness(&audit, &context_hit.entry.id, ContextMapFreshness::Current,),
        Some(ContextMapFreshness::Stale)
    );
    assert_eq!(
        audited_verification(result.data[0].entry.value.verification, freshness),
        BlackboardVerification::Stale
    );
    let unchecked_audit =
        observe_evidence(&services, "project-1", &[], [context_hit.entry.id.clone()]).await;
    let unchecked = audited_blackboard_freshness(&result.data[0], Some(&unchecked_audit));
    assert_eq!(unchecked, AuditedEvidenceFreshness::UncheckedThisTurn);
    assert_eq!(
        audited_verification(result.data[0].entry.value.verification, unchecked),
        BlackboardVerification::Unverified
    );
    let node_after = hierarchy
        .get_node("project-1", &context_hit.entry.value.node_id)
        .await
        .expect("node lookup")
        .expect("source node");
    let revision_after = blackboard
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 1,
        })
        .await
        .expect("root projection")
        .revision;
    assert_eq!(node_after, node_before);
    assert_eq!(revision_after, revision_before);
}
use codex_project_intelligence::NewBlackboardEntry;
