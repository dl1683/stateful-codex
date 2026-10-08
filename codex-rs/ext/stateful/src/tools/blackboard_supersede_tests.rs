use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use codex_project_intelligence::SupersededEntry;
use codex_state::SqliteConfig;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::SupersedeReference;
use super::committed_succession;
use crate::services::ProjectIntelligenceServices;
use crate::visible_root::VisibleRoot;
use crate::visible_root::VisibleRootRegistry;

const PROJECT_ID: &str = "project-1";

/// A retry must name each committed predecessor exactly once; an alias reused by a later
/// packet for another entry still matches the predecessor it named when the call ran, and
/// all aliases of one call resolve against the same packet record.
#[tokio::test]
async fn retries_must_cover_the_committed_replacement_exactly_once() {
    let state_home = TempDir::new().expect("state home");
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(state_home.path().abs()));
    let node_id = services
        .project_node_id(PROJECT_ID)
        .await
        .expect("project node");
    let store = services.blackboard().await.expect("blackboard");
    let value = |content: &str| NewBlackboardEntry {
        project_id: PROJECT_ID.to_string(),
        node_id: node_id.clone(),
        kind: BlackboardKind::Decision,
        content: content.to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(9_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::High,
        root_promotion: RootPromotion::NotPromoted,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-1".to_string(),
        },
    };
    let a = BlackboardEntryId::parse("a").expect("ID");
    let b = BlackboardEntryId::parse("b").expect("ID");
    let x = BlackboardEntryId::parse("x").expect("ID");
    store.create_entry(a.clone(), value("A.")).await.expect("A");
    store.create_entry(b.clone(), value("B.")).await.expect("B");
    store
        .create_successor(
            x.clone(),
            value("A and B."),
            vec![
                SupersededEntry {
                    id: a.clone(),
                    expected_revision: 1,
                },
                SupersededEntry {
                    id: b.clone(),
                    expected_revision: 1,
                },
            ],
        )
        .await
        .expect("merge");
    // The packet showed E1 = A when the call ran; a later full packet shows E1 = X.
    let registry = VisibleRootRegistry::default();
    let mut earlier = VisibleRoot::new(1);
    earlier.insert("a".to_string(), "E1".to_string(), 1);
    earlier.insert("z".to_string(), "E2".to_string(), 1);
    registry.record("thread-1", earlier);
    registry.clear("thread-1");
    let mut later = VisibleRoot::new(2);
    later.insert("x".to_string(), "E1".to_string(), 1);
    later.insert("b".to_string(), "E2".to_string(), 1);
    registry.record("thread-1", later);

    let entry = |id: &str| SupersedeReference::Entry {
        entry_id: id.to_string(),
        revision: 1,
    };
    let alias = || SupersedeReference::Alias {
        alias: "E1".to_string(),
    };
    let mut outcomes = Vec::new();
    let mut retry_value = value("A and B.");
    retry_value.provenance.source_id = "turn-2".to_string();
    for references in [
        vec![entry("a"), entry("b")],
        vec![alias(), entry("b")],
        vec![entry("a"), entry("a")],
        vec![alias(), entry("a")],
        vec![entry("a")],
        // E1 meant A only in the earlier record and E2 meant B only in the later one; one
        // call never saw both, so mixing the records is refused.
        vec![
            alias(),
            SupersedeReference::Alias {
                alias: "E2".to_string(),
            },
        ],
    ] {
        outcomes.push(
            committed_succession(
                store,
                &registry,
                PROJECT_ID,
                "thread-1",
                &x,
                &retry_value,
                &references,
            )
            .await
            .is_ok_and(|found| found.is_some()),
        );
    }
    assert_eq!(outcomes, vec![true, true, false, false, false, false]);

    // Public/host successions do not authorize model replay of non-Agent predecessors.
    for (origin, authority) in [
        (BlackboardProvenanceKind::User, None),
        (
            BlackboardProvenanceKind::User,
            Some(codex_project_intelligence::KnowledgeAuthority::AssistantReported),
        ),
        (
            BlackboardProvenanceKind::User,
            Some(codex_project_intelligence::KnowledgeAuthority::HumanDirect),
        ),
        (BlackboardProvenanceKind::Import, None),
        (
            BlackboardProvenanceKind::Import,
            Some(codex_project_intelligence::KnowledgeAuthority::AssistantReported),
        ),
        (BlackboardProvenanceKind::Maintenance, None),
        (
            BlackboardProvenanceKind::Agent,
            Some(codex_project_intelligence::KnowledgeAuthority::LegacyUnknown),
        ),
    ] {
        let predecessor_id =
            BlackboardEntryId::parse(format!("protected-{origin:?}-{authority:?}"))
                .expect("predecessor ID");
        let successor_id = BlackboardEntryId::parse(format!("successor-{origin:?}-{authority:?}"))
            .expect("successor ID");
        let mut user_value = value(&format!(
            "User's corrected words and reason {origin:?}-{authority:?}."
        ));
        user_value.provenance.kind = origin;
        let mut predecessor = store
            .create_entry(predecessor_id.clone(), user_value)
            .await
            .expect("user predecessor");
        if origin == BlackboardProvenanceKind::Import && authority.is_some() {
            // Seed an old binary's origin transfer before the committed succession.
            predecessor = store
                .update_entry(
                    PROJECT_ID,
                    &predecessor_id,
                    codex_project_intelligence::BlackboardEntryUpdate {
                        expected_revision: predecessor.revision,
                        kind: BlackboardKind::Fact,
                        content: predecessor.value.content.clone(),
                        structured_value: predecessor.value.structured_value.clone(),
                        confidence: predecessor.value.confidence,
                        verification: predecessor.value.verification,
                        importance: predecessor.value.importance,
                        root_promotion: predecessor.value.root_promotion,
                        evidence: predecessor.value.evidence.clone(),
                        premises: predecessor.value.premises.clone(),
                        state: codex_project_intelligence::BlackboardEntryState::Active,
                        superseded_by: None,
                        provenance: value("old-model-call").provenance,
                    },
                )
                .await
                .unwrap();
        }
        if let Some(authority) = authority {
            store
                .record_context(
                    &predecessor,
                    &codex_project_intelligence::KnowledgeContext::new(
                        codex_project_intelligence::KnowledgeCategory::Decision,
                        authority,
                    ),
                    /*change*/ None,
                )
                .await
                .expect("historical authority");
        }
        let successor = store
            .create_successor(
                successor_id.clone(),
                value("Historical model successor."),
                vec![SupersededEntry {
                    id: predecessor_id.clone(),
                    expected_revision: predecessor.revision,
                }],
            )
            .await
            .expect("historical succession");
        let before = store
            .get_entry(PROJECT_ID, &predecessor_id)
            .await
            .expect("predecessor");
        let error = committed_succession(
            store,
            &registry,
            PROJECT_ID,
            "thread-1",
            &successor_id,
            &successor.successor.value,
            &[SupersedeReference::Entry {
                entry_id: predecessor_id.to_string(),
                revision: predecessor.revision,
            }],
        )
        .await
        .expect_err("User-provenance replay refused");
        assert!(error.to_string().contains("memory"));
        assert_eq!(
            store
                .get_entry(PROJECT_ID, &predecessor_id)
                .await
                .expect("unchanged"),
            before
        );
        assert_eq!(
            store
                .get_entry(PROJECT_ID, &successor_id)
                .await
                .expect("successor"),
            Some(successor.successor)
        );
    }
}
