use codex_extension_api::ContextualUserFragment;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::NewHierarchyNode;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::ProjectRelativePath;
use codex_project_intelligence::RootPromotion;
use codex_protocol::user_input::UserInput;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::MAX_FRAGMENT_BYTES;
use super::ReuseCandidate;
use super::question_text;
use super::reuse_candidate_slate;
use crate::services::ProjectIntelligenceServices;

const PROJECT_ID: &str = "project-1";

async fn fixture() -> (TempDir, ProjectIntelligenceServices, HierarchyNodeId) {
    let temp_dir = TempDir::new().expect("tempdir created");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(temp_dir.path().abs()));
    let project_node = HierarchyNodeId::parse("node-project").expect("valid node ID");
    services
        .hierarchy()
        .await
        .expect("hierarchy opens")
        .create_node(
            project_node.clone(),
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
    (temp_dir, services, project_node)
}

async fn insert_entry(
    services: &ProjectIntelligenceServices,
    node_id: &HierarchyNodeId,
    id: &str,
    kind: BlackboardKind,
    importance: BlackboardImportance,
    content: &str,
) {
    services
        .blackboard()
        .await
        .expect("blackboard opens")
        .create_entry(
            BlackboardEntryId::parse(id).expect("valid entry ID"),
            NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: node_id.clone(),
                kind,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(8_000).expect("valid confidence"),
                verification: BlackboardVerification::Unverified,
                importance,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
                    source_id: format!("turn-{id}"),
                },
            },
        )
        .await
        .expect("entry inserts");
}

#[tokio::test]
async fn slate_combines_lexical_matches_with_one_high_priority_failure() {
    let (_temp_dir, services, project_node) = fixture().await;
    insert_entry(
        &services,
        &project_node,
        "failure-reset",
        BlackboardKind::Failure,
        BlackboardImportance::Critical,
        "Reset synchronizer mismatch invalidated the apparently strongest approach.",
    )
    .await;
    for (id, content) in [
        (
            "finding-a",
            "The deployment lifecycle boundary begins after review completes.",
        ),
        (
            "finding-b",
            "A lifecycle boundary separates deployment preparation from execution.",
        ),
        (
            "finding-c",
            "Deployment ownership changes at the documented boundary.",
        ),
    ] {
        insert_entry(
            &services,
            &project_node,
            id,
            BlackboardKind::Fact,
            BlackboardImportance::Normal,
            content,
        )
        .await;
    }

    let slate = reuse_candidate_slate(
        PROJECT_ID,
        &services,
        &[UserInput::Text {
            text: "Which deployment lifecycle boundary controls the handoff?".to_string(),
            text_elements: Vec::new(),
        }],
    )
    .await
    .expect("slate resolves")
    .expect("slate is available");

    assert_eq!(
        slate.candidates,
        vec![
            ReuseCandidate {
                alias: "E2".to_string(),
                content: "The deployment lifecycle boundary begins after review completes."
                    .to_string(),
            },
            ReuseCandidate {
                alias: "E4".to_string(),
                content: "Deployment ownership changes at the documented boundary.".to_string(),
            },
            ReuseCandidate {
                alias: "E3".to_string(),
                content: "A lifecycle boundary separates deployment preparation from execution."
                    .to_string(),
            },
            ReuseCandidate {
                alias: "E1".to_string(),
                content:
                    "Reset synchronizer mismatch invalidated the apparently strongest approach."
                        .to_string(),
            },
        ]
    );
    let body = slate.body();
    assert!(body.len() <= MAX_FRAGMENT_BYTES);
    assert!(body.contains("Attention cue only"));
    assert!(body.contains("Project ID: project-1"));
    assert!(body.contains("E1"));
}

#[test]
fn slate_body_preserves_maximum_project_identity_within_its_hard_bound() {
    let project_id = "p".repeat(512);
    let slate = super::ReuseCandidateSlate {
        project_id: project_id.clone(),
        root_revision: u64::MAX,
        candidates: (1..=4)
            .map(|index| ReuseCandidate {
                alias: format!("E{index}"),
                content: "x".repeat(96),
            })
            .collect(),
    };

    let body = slate.body();
    assert!(body.len() <= MAX_FRAGMENT_BYTES);
    assert!(body.contains(&format!("Project ID: {project_id}")));
}

#[test]
fn question_text_is_trimmed_and_bounded_for_blackboard_queries() {
    let text = format!("  {}  ", "question ".repeat(256));
    let question = question_text(&[UserInput::Text {
        text,
        text_elements: Vec::new(),
    }])
    .expect("question is present");

    assert_eq!(question, question.trim());
    assert!(question.len() <= 1024);
    assert!(question.is_char_boundary(question.len()));
}
