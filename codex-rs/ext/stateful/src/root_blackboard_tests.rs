use codex_project_intelligence::BlackboardEntry;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardHit;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::ConfidenceScore;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::NewBlackboardEntry;
use codex_project_intelligence::RootBlackboardProjection;
use codex_project_intelligence::RootPromotion;
use pretty_assertions::assert_eq;

use super::ResolvedRootBlackboard;
use super::RootBlackboardStatus;
use super::USER_RULES_HEADER;
use super::render_root_blackboard;

fn hit(
    id: &str,
    kind: BlackboardKind,
    provenance: BlackboardProvenanceKind,
    content: &str,
) -> BlackboardHit {
    BlackboardHit::new(
        BlackboardEntry {
            id: BlackboardEntryId::parse(id).expect("entry ID"),
            value: NewBlackboardEntry {
                project_id: "project-1".to_string(),
                node_id: HierarchyNodeId::parse("node-root").expect("node ID"),
                kind,
                content: content.to_string(),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(10_000).expect("confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: provenance,
                    source_id: "turn-1".to_string(),
                },
            },
            state: BlackboardEntryState::Active,
            superseded_by: None,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
        },
        BlackboardEvidenceFreshness::NotApplicable,
    )
}

/// User rules lead the root under their own heading; an agent-recorded rule (tui7's
/// merged entry with a one-off "no code changes" restriction) is counted but never shown.
#[test]
fn user_rules_lead_and_agent_recorded_rules_are_not_applied() {
    let status = RootBlackboardStatus::Available(ResolvedRootBlackboard {
        projection: RootBlackboardProjection {
            project_id: "project-1".to_string(),
            revision: 3,
            data: vec![
                hit(
                    "rule-next",
                    BlackboardKind::Instruction,
                    BlackboardProvenanceKind::User,
                    "End each of your replies with a line starting with 'Next:'.",
                ),
                hit(
                    "agent-rule",
                    BlackboardKind::Instruction,
                    BlackboardProvenanceKind::Agent,
                    "No code changes during the initial orientation and planning pass.",
                ),
                hit(
                    "recipe",
                    BlackboardKind::Fact,
                    BlackboardProvenanceKind::Agent,
                    "Recipe: run tests with PYTHONPATH=src python -m pytest tests/test_x.py.",
                ),
            ],
            omitted_entries: 0,
            candidate_entries: 0,
        },
        evidence_routes: Default::default(),
        evidence_audit: None,
    });
    let mut output = String::new();
    render_root_blackboard(&mut output, &status);
    let headings = output
        .lines()
        .filter(|line| !line.starts_with("- E") && !line.starts_with("For durableLearning"))
        .collect::<Vec<_>>();
    assert_eq!(
        (
            headings,
            output.contains("Next:"),
            output.contains("No code changes"),
            output.contains("PYTHONPATH=src"),
        ),
        (
            vec![
                "Project intelligence revision: 3",
                "Root blackboard (active, explicitly promoted knowledge):",
                USER_RULES_HEADER,
                "Other promoted knowledge:",
                "- 1 agent-recorded rules are not applied: they are not the user's own words. blackboard_query lists them; treat them as unconfirmed.",
            ],
            true,
            false,
            true,
        )
    );
}
