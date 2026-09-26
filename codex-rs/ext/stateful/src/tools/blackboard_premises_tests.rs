use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardPremiseLink;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::PremiseArguments;
use super::resolve_premises;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";

#[tokio::test]
async fn premise_resolution_rejects_source_bytes_changed_after_indexing() {
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
        project_id: PROJECT_ID.to_string(),
        roots: vec![project_root.path().to_path_buf()],
    })
    .await
    .expect("index source");
    let context_hit = services
        .context_map()
        .await
        .expect("context map")
        .file_hits_for_path(
            PROJECT_ID,
            &ProjectRelativePath::parse("policy.md").expect("relative path"),
        )
        .await
        .expect("source lookup")
        .into_iter()
        .next()
        .expect("indexed source");
    let premise_id = BlackboardEntryId::parse("policy-threshold").expect("entry ID");
    let premise = services
        .blackboard()
        .await
        .expect("blackboard")
        .create_entry(
            premise_id.clone(),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: context_hit.entry.value.node_id,
                kind: BlackboardKind::Number,
                content: "The policy threshold is 10.".to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::SourceVerified,
                importance: BlackboardImportance::Critical,
                root_promotion: RootPromotion::Promoted,
                evidence: vec![BlackboardEvidenceLink {
                    context_map_entry_id: context_hit.entry.id,
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
        .expect("create premise");
    let arguments = || {
        vec![PremiseArguments {
            entry_id: premise_id.to_string(),
            revision: premise.revision,
        }]
    };
    assert_eq!(
        resolve_premises(
            PROJECT_ID,
            &services,
            &[project_root.path().to_path_buf()],
            arguments(),
        )
        .await
        .expect("current premise resolves"),
        vec![BlackboardPremiseLink {
            entry_id: premise_id.clone(),
            revision: premise.revision,
        }]
    );

    std::fs::write(&source_path, "threshold=60\n").expect("change source without reindexing");
    let result = resolve_premises(
        PROJECT_ID,
        &services,
        &[project_root.path().to_path_buf()],
        arguments(),
    )
    .await;
    let Err(error) = result else {
        panic!("live source change must reject premise reuse");
    };
    assert!(
        error
            .to_string()
            .contains("not currently source-verified or user-confirmed")
    );
}
