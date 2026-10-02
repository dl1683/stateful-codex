use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootBlackboardQuery;
use codex_project_intelligence::RootPromotion;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::capture_marked_rules;
use crate::services::ProjectIntelligenceServices;
use crate::user_rules::RuleStanding;

const MESSAGE: &str = "A couple of ways I like to work: please never run git commit yourself. No code changes yet. Never run migrations during this pass. End each of your replies with a line starting with 'Next:'.";

#[tokio::test]
async fn marked_rules_are_stored_once_in_the_users_words() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));

    let first = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        MESSAGE,
    )
    .await;
    let again = capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-2",
        "turn-7",
        MESSAGE,
    )
    .await;

    let summarize = |captured: &[super::CapturedRule]| {
        captured
            .iter()
            .map(|rule| {
                (
                    rule.entry.value.content.clone(),
                    rule.entry.value.root_promotion,
                    rule.entry.value.provenance.kind,
                    rule.entry.value.provenance.source_id.clone(),
                    rule.standing,
                    rule.newly_stored,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = |newly_stored: bool| {
        vec![
            (
                "A couple of ways I like to work: please never run git commit yourself."
                    .to_string(),
                RootPromotion::Promoted,
                BlackboardProvenanceKind::User,
                "user-message:thread-1/turn-1".to_string(),
                RuleStanding::Standing,
                newly_stored,
            ),
            (
                "Never run migrations during this pass.".to_string(),
                RootPromotion::Candidate,
                BlackboardProvenanceKind::User,
                "user-message:thread-1/turn-1".to_string(),
                RuleStanding::Pending,
                newly_stored,
            ),
            (
                "End each of your replies with a line starting with 'Next:'.".to_string(),
                RootPromotion::Promoted,
                BlackboardProvenanceKind::User,
                "user-message:thread-1/turn-1".to_string(),
                RuleStanding::Standing,
                newly_stored,
            ),
        ]
    };
    assert_eq!(
        (summarize(&first), summarize(&again)),
        (expected(true), expected(false))
    );

    // Only the standing rules reach the root, and they come first.
    let root = services
        .blackboard()
        .await
        .expect("blackboard")
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 256,
        })
        .await
        .expect("root");
    assert_eq!(
        (root.data.len(), root.candidate_entries),
        (2, 1),
        "{root:?}"
    );
}

/// horizon1: rules must stay in the packet as memory grows. User rules are selected before
/// any other promoted entry, so the root limit and byte budget cannot push them out.
#[tokio::test]
async fn user_rules_are_selected_before_more_important_knowledge() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services
        .project_node_id("project-1")
        .await
        .expect("project node");
    let store = services.blackboard().await.expect("blackboard");
    for index in 0..3 {
        store
            .create_entry(
                codex_project_intelligence::BlackboardEntryId::parse(format!("a-critical-{index}"))
                    .expect("entry ID"),
                codex_project_intelligence::NewBlackboardEntry {
                    project_id: "project-1".to_string(),
                    node_id: node_id.clone(),
                    kind: codex_project_intelligence::BlackboardKind::Fact,
                    content: format!("Critical fact {index}."),
                    structured_value: None,
                    confidence: codex_project_intelligence::ConfidenceScore::from_basis_points(
                        9_000,
                    )
                    .expect("confidence"),
                    verification: codex_project_intelligence::BlackboardVerification::Unverified,
                    importance: codex_project_intelligence::BlackboardImportance::Critical,
                    root_promotion: RootPromotion::Promoted,
                    evidence: Vec::new(),
                    premises: Vec::new(),
                    provenance: codex_project_intelligence::BlackboardProvenance {
                        kind: BlackboardProvenanceKind::Agent,
                        source_id: "turn-0".to_string(),
                    },
                },
            )
            .await
            .expect("fact stored");
    }
    capture_marked_rules(
        &services,
        /*event_sink*/ None,
        "project-1",
        "thread-1",
        "turn-1",
        "From now on, run only the tests relevant to the change.",
    )
    .await;
    let root = store
        .root_projection(RootBlackboardQuery {
            project_id: "project-1".to_string(),
            max_entries: 1,
        })
        .await
        .expect("root");
    assert_eq!(
        root.data
            .iter()
            .map(|hit| hit.entry.value.content.as_str())
            .collect::<Vec<_>>(),
        vec!["From now on, run only the tests relevant to the change."]
    );
}
