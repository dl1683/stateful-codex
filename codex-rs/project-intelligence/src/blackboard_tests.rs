use pretty_assertions::assert_eq;

use super::*;

fn evidence(id: &str) -> BlackboardEvidenceLink {
    BlackboardEvidenceLink {
        context_map_entry_id: ContextMapEntryId::parse(id).expect("valid context-map ID"),
        source_fingerprint: SourceFingerprint::parse("sha256:source").expect("valid fingerprint"),
        line_range: Some(EvidenceLineRange { start: 4, end: 7 }),
    }
}

fn entry() -> NewBlackboardEntry {
    NewBlackboardEntry {
        project_id: "project-1".to_string(),
        node_id: HierarchyNodeId::parse("node-file").expect("valid node ID"),
        kind: BlackboardKind::Number,
        content: "Quarterly revenue was USD 14.2 million.".to_string(),
        structured_value: Some(BlackboardStructuredValue {
            value: "14200000".to_string(),
            unit: Some("USD".to_string()),
        }),
        confidence: ConfidenceScore::from_basis_points(9_500).expect("valid confidence"),
        verification: BlackboardVerification::SourceVerified,
        importance: BlackboardImportance::High,
        root_promotion: RootPromotion::Candidate,
        evidence: vec![evidence("map-report")],
        premises: Vec::new(),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Agent,
            source_id: "turn-1".to_string(),
        },
    }
}

#[test]
fn entry_contract_preserves_epistemic_and_promotion_state() {
    let value = entry();
    assert_eq!(value.validate(), Ok(()));
    assert_eq!(value.confidence.basis_points(), 9_500);
    assert_eq!(value.verification, BlackboardVerification::SourceVerified);
    assert_eq!(value.root_promotion, RootPromotion::Candidate);
}

#[test]
fn source_verified_entries_require_unique_evidence() {
    let mut value = entry();
    value.evidence.clear();
    assert_eq!(
        value.validate(),
        Err(BlackboardError::VerifiedWithoutEvidence)
    );

    let mut second_region = evidence("map-report");
    second_region.line_range = Some(EvidenceLineRange { start: 12, end: 18 });
    value.evidence = vec![evidence("map-report"), second_region];
    assert_eq!(value.validate(), Ok(()));

    value.evidence.push(evidence("map-report"));
    assert_eq!(
        value.validate(),
        Err(BlackboardError::DuplicateEvidenceLink)
    );
}

#[test]
fn evidence_line_ranges_are_bounded_and_ordered() {
    let mut value = entry();
    value.evidence[0].line_range = Some(EvidenceLineRange { start: 0, end: 1 });
    assert_eq!(
        value.validate(),
        Err(BlackboardError::InvalidEvidenceLineRange)
    );

    value.evidence[0].line_range = Some(EvidenceLineRange {
        start: 1,
        end: 2_001,
    });
    assert_eq!(
        value.validate(),
        Err(BlackboardError::InvalidEvidenceLineRange)
    );
}

#[test]
fn entry_content_and_structured_values_are_bounded() {
    let mut value = entry();
    value.content = " padded ".to_string();
    assert_eq!(value.validate(), Err(BlackboardError::InvalidContent));

    let mut value = entry();
    value.structured_value = Some(BlackboardStructuredValue {
        value: "14.2".to_string(),
        unit: Some("USD\nforged".to_string()),
    });
    assert_eq!(value.validate(), Err(BlackboardError::InvalidUnit));
    assert_eq!(
        ConfidenceScore::from_basis_points(10_001),
        Err(BlackboardError::InvalidConfidence)
    );
}

#[test]
fn supersession_requires_a_distinct_successor() {
    let value = entry();
    let entry_id = BlackboardEntryId::parse("entry-1").expect("valid entry ID");
    let mut update = BlackboardEntryUpdate {
        expected_revision: 1,
        kind: value.kind,
        content: value.content,
        structured_value: value.structured_value,
        confidence: value.confidence,
        verification: value.verification,
        importance: value.importance,
        root_promotion: RootPromotion::Promoted,
        evidence: value.evidence,
        premises: value.premises,
        state: BlackboardEntryState::Superseded,
        superseded_by: Some(BlackboardEntryId::parse("entry-2").expect("valid successor")),
        provenance: BlackboardProvenance {
            kind: BlackboardProvenanceKind::Maintenance,
            source_id: "maintenance-1".to_string(),
        },
    };
    assert_eq!(update.validate(&entry_id), Ok(()));

    update.superseded_by = Some(entry_id.clone());
    assert_eq!(
        update.validate(&entry_id),
        Err(BlackboardError::InvalidSupersession)
    );
    update.state = BlackboardEntryState::Active;
    assert_eq!(
        update.validate(&entry_id),
        Err(BlackboardError::InvalidSupersession)
    );
}
