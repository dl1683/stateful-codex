//! Aggregate size of the Stateful developer content a fresh window carries for a realistic
//! project: the project root, the run packet, and every other Stateful section.

use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::WorldStateSectionContribution;
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
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::NewSteeringInstruction;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::SteeringId;
use codex_stateful_runtime::SteeringStatus;
use codex_stateful_runtime::WorkflowMode;
use codex_thread_store::StoredProject;
use codex_thread_store::StoredProjectRoot;

use crate::continuity::CapturedTurn;
use crate::continuity::ContinuityRecord;
use crate::continuity::continuity_world_state_section;
use crate::root_blackboard::ResolvedRootBlackboard;
use crate::root_blackboard::RootBlackboardStatus;
use crate::run_world_state::RunWorldStateStatus;
use crate::run_world_state::run_world_state_section;
use crate::world_state::ProjectIntelligenceStatus;
use crate::world_state::project_world_state_section;

/// Rendered bytes (markers included) of the fixture's Stateful content at a window start.
/// Measured 2026-10-02 for Collaborative: project plus run 11,659 bytes before the trim and
/// 8,370 after; with five populated outcomes 14,234 (project 6,229, run 2,141, outcomes 5,864).
/// The continuity record replaced the outcomes: 14,508 (continuity 6,138, ten long turns).
const MAX_FIXTURE_PACKET_BYTES: usize = 22_000;

const PROJECT_ID: &str = "project-1";

fn promoted_entry(index: usize) -> BlackboardHit {
    BlackboardHit::new(
        BlackboardEntry {
            id: BlackboardEntryId::parse(format!("entry-{index:02}")).expect("valid entry ID"),
            value: NewBlackboardEntry {
                project_id: PROJECT_ID.to_string(),
                node_id: HierarchyNodeId::parse("node-root").expect("valid node ID"),
                kind: BlackboardKind::Decision,
                content: format!(
                    "Decision {index}: the scaler keeps metric units, rounds eggs to whole numbers with a minimum of one, and leaves recipes.json edits to the user unless asked; measured on the current checkout with the focused tests in tests/test_scale.py passing."
                ),
                structured_value: None,
                confidence: ConfidenceScore::from_basis_points(8_500).expect("valid confidence"),
                verification: BlackboardVerification::Unverified,
                importance: BlackboardImportance::High,
                root_promotion: RootPromotion::Promoted,
                evidence: Vec::new(),
                premises: Vec::new(),
                provenance: BlackboardProvenance {
                    kind: BlackboardProvenanceKind::Agent,
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

fn project_status() -> ProjectIntelligenceStatus {
    ProjectIntelligenceStatus::Available {
        project: Box::new(StoredProject {
            id: PROJECT_ID.to_string(),
            name: "Recipe scaler".to_string(),
            roots: vec![StoredProjectRoot {
                path: "C:\\work\\recipes".to_string(),
            }],
            metadata: Default::default(),
            position: 0,
            created_at_ms: 1,
            updated_at_ms: 2,
            recency_at_ms: None,
        }),
        last_refresh: None,
        root_blackboard: Box::new(RootBlackboardStatus::Available(ResolvedRootBlackboard {
            projection: RootBlackboardProjection {
                project_id: PROJECT_ID.to_string(),
                revision: 12,
                data: (1..=10).map(promoted_entry).collect(),
                omitted_entries: 0,
                candidate_entries: 0,
            },
            evidence_routes: Default::default(),
            evidence_audit: None,
        })),
    }
}

fn run_status(mode: WorkflowMode) -> RunWorldStateStatus {
    let run_id = StatefulRunId::parse("run-1").expect("valid run id");
    RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: run_id.clone(),
            value: NewStatefulRun {
                project_id: PROJECT_ID.to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Fix the scaler so eggs round to whole numbers and add the crepes recipe in metric units. Remember: metric units only; never change recipes.json without asking.".to_string(),
                mode,
                budget: RunBudget {
                    max_continuations: 24,
                    max_elapsed_seconds: 14_400,
                },
            },
            status: StatefulRunStatus::Running,
            strategy: Some(
                "Fix rounding first, then convert the recipe, then ask before writing recipes.json."
                    .to_string(),
            ),
            strategy_revision: 1,
            result: None,
            continuations_used: 0,
            revision: 3,
            created_at_ms: 1,
            updated_at_ms: 3,
        }),
        obligation: Some(Box::new(StatefulObligation {
            id: "obligation-1".to_string(),
            value: NewObligation {
                project_id: PROJECT_ID.to_string(),
                run_id: run_id.clone(),
                packet: ObligationPacket {
                    learning: vec![
                        "Eggs were scaled as floats; rounding now happens at output.".to_string(),
                        "The crepes recipe needs gram conversions for flour and sugar.".to_string(),
                    ],
                    next: vec!["Ask before writing recipes.json.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "turn-2".to_string(),
            },
            sequence: 1,
            revision: 1,
            created_at_ms: 2,
        })),
        steering: (1..=2)
            .map(|index| StatefulSteering {
                id: SteeringId::parse(format!("steering-{index}")).expect("valid steering id"),
                value: NewSteeringInstruction {
                    project_id: PROJECT_ID.to_string(),
                    run_id: run_id.clone(),
                    input: "Keep the egg rounding change separate from the recipe change."
                        .to_string(),
                    affected_obligation_ids: Vec::new(),
                },
                status: SteeringStatus::Submitted,
                resulting_strategy_revision: None,
                reason: None,
                revision: 1,
                created_at_ms: 2,
                updated_at_ms: 2,
            })
            .collect(),
        steering_complete: true,
        checkpoint_due: Some(1),
    }
}

/// Ten captured turns with long requests and answers, as the continuity record gathers them.
fn continuity_record() -> ContinuityRecord {
    ContinuityRecord {
        project_id: PROJECT_ID.to_string(),
        captured_at_ms: 1_759_341_720_000,
        turns: (1..=10)
            .map(|index| CapturedTurn {
                thread_id: format!("01a0fbad-0c72-7143-8052-63fab09364{index:02}"),
                thread_title: Some("Fix the recipe scaler".to_string()),
                current_thread: false,
                turn_id: format!("01a0fbad-1689-75e3-ac68-867cb758f5{index:02}"),
                at_ms: Some(1_759_341_720_000 - i64::from(index) * 60_000),
                unfinished_status: None,
                user: Some(
                    "Add my grandma's crepes, convert them to metric, and keep eggs whole. "
                        .repeat(30),
                ),
                answer: Some(
                    "May I modify recipes.json with flour 125 g, milk 300 ml and two eggs? "
                        .repeat(60),
                ),
            })
            .collect(),
        more_turns: true,
        unreadable_threads: 0,
        history_unavailable: false,
    }
}

fn rendered_bytes(section: WorldStateSectionContribution) -> usize {
    section
        .render_diff(PreviousWorldStateSection::Absent)
        .map_or(0, |fragment| {
            let (start, end) = fragment.markers();
            start.len() + fragment.body().len() + end.len()
        })
}

#[test]
fn fresh_window_stateful_content_stays_within_its_budget() {
    for mode in [
        WorkflowMode::Collaborative,
        WorkflowMode::Autonomous,
        WorkflowMode::Socratic,
    ] {
        let sections = [
            rendered_bytes(project_world_state_section(
                project_status(),
                /*visible_root*/ None,
            )),
            rendered_bytes(run_world_state_section(run_status(mode))),
            rendered_bytes(continuity_world_state_section(&continuity_record())),
        ];
        let total = sections.iter().sum::<usize>();

        assert!(
            total <= MAX_FIXTURE_PACKET_BYTES,
            "{mode:?} Stateful content is {total} bytes: {sections:?}"
        );
    }
}
