use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEntryUpdate;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ChangeOperation;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootPromotion;
use pretty_assertions::assert_eq;

use super::update_operation;

fn before() -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: "project-1".to_string(),
        node_id: HierarchyNodeId::parse("node").expect("node"),
        kind: BlackboardKind::Decision,
        content: "Use SQLite.".to_string(),
        structured_value: None,
        confidence: ConfidenceScore::from_basis_points(8_000).expect("confidence"),
        verification: BlackboardVerification::Unverified,
        importance: BlackboardImportance::Normal,
        root_promotion: RootPromotion::Candidate,
        evidence: Vec::new(),
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "agent".to_string(),
        },
    }
}

fn unchanged(value: &NewBlackboardEntry) -> BlackboardEntryUpdate {
    BlackboardEntryUpdate {
        expected_revision: 1,
        kind: value.kind,
        content: value.content.clone(),
        structured_value: value.structured_value.clone(),
        confidence: value.confidence,
        verification: value.verification,
        importance: value.importance,
        root_promotion: value.root_promotion,
        evidence: value.evidence.clone(),
        premises: value.premises.clone(),
        state: BlackboardEntryState::Active,
        superseded_by: None,
        provenance: value.provenance.clone(),
    }
}

/// Every revision that changes something is journaled once with what it did; a revision
/// that changes nothing is not.
#[test]
fn model_revisions_are_classified() {
    let value = before();
    let with = |change: fn(&mut BlackboardEntryUpdate)| {
        let mut update = unchanged(&value);
        change(&mut update);
        update_operation(&value, &update)
    };
    assert_eq!(
        vec![
            with(|_| {}),
            with(|update| update.state = BlackboardEntryState::Tombstoned),
            with(|update| update.state = BlackboardEntryState::Superseded),
            with(|update| update.content = "Use Postgres.".to_string()),
            with(|update| update.verification = BlackboardVerification::Stale),
            with(|update| update.verification = BlackboardVerification::Disputed),
            with(|update| update.root_promotion = RootPromotion::Promoted),
            with(|update| update.root_promotion = RootPromotion::NotPromoted),
            with(|update| update.importance = BlackboardImportance::High),
            with(|update| update.verification = BlackboardVerification::SourceVerified),
        ],
        vec![
            None,
            Some(ChangeOperation::Forgotten),
            Some(ChangeOperation::Invalidated),
            Some(ChangeOperation::Corrected),
            Some(ChangeOperation::Invalidated),
            Some(ChangeOperation::Invalidated),
            Some(ChangeOperation::Promoted),
            Some(ChangeOperation::Corrected),
            Some(ChangeOperation::Corrected),
            Some(ChangeOperation::Corrected),
        ]
    );
}
