use codex_extension_api::PreviousWorldStateSection;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootBlackboardProjection;
use codex_stateful_runtime::WorkflowMode;
use pretty_assertions::assert_eq;

use super::MAX_CONTINUATION_PROJECT_BYTES;
use super::continuation_project;
use super::continuation_project_section;
use crate::packet_budget_tests::background_entry;
use crate::packet_budget_tests::project_status;
use crate::packet_budget_tests::promoted_entry;
use crate::packet_budget_tests::run_status;
use crate::root_blackboard::ResolvedRootBlackboard;
use crate::root_blackboard::RootBlackboardStatus;
use crate::run_world_state::run_continuation_section;
use crate::visible_root::VisibleRootRegistry;
use crate::world_state::ProjectIntelligenceStatus;

fn user_rule(index: usize, text: &str) -> codex_project_intelligence::BlackboardHit {
    let mut hit = promoted_entry(index);
    hit.entry.value.kind = BlackboardKind::Instruction;
    hit.entry.value.content = text.to_string();
    hit.entry.value.provenance = BlackboardProvenance {
        kind: BlackboardProvenanceKind::User,
        source_id: "user-message:thread-1/turn-1".to_string(),
    };
    hit
}

fn status_with(data: Vec<codex_project_intelligence::BlackboardHit>) -> ProjectIntelligenceStatus {
    let ProjectIntelligenceStatus::Available {
        project,
        last_refresh,
        ..
    } = project_status()
    else {
        unreachable!("fixture is available");
    };
    ProjectIntelligenceStatus::Available {
        project,
        last_refresh,
        root_blackboard: Box::new(RootBlackboardStatus::Available(
            ResolvedRootBlackboard::new(
                RootBlackboardProjection {
                    project_id: "project-1".to_string(),
                    revision: 12,
                    data,
                    omitted_entries: 0,
                    candidate_entries: 0,
                },
                Default::default(),
                None,
            ),
        )),
    }
}

fn rendered(
    section: &codex_extension_api::WorldStateSectionContribution,
    previous: PreviousWorldStateSection<'_>,
) -> Option<String> {
    section
        .render_diff(previous)
        .map(|fragment| fragment.body().to_string())
}

#[test]
fn continuation_keeps_rules_and_background_but_not_other_knowledge() {
    let status = status_with(vec![
        promoted_entry(1),
        user_rule(
            2,
            "Metric units only; never change recipes.json without asking.",
        ),
        promoted_entry(3),
        background_entry(),
    ]);
    let (full_body, _) = status.render();
    let project = continuation_project(&status).expect("rules fit");
    let registry = VisibleRootRegistry::default();
    let section = continuation_project_section(project, (registry.clone(), "thread-1".to_string()));
    let body = rendered(&section, PreviousWorldStateSection::Absent).expect("rendered");

    // The rule keeps its alias from the full packet, so completion resolves the same entry.
    assert!(body.contains("- E2"), "{body}");
    assert!(body.contains("Metric units only"), "{body}");
    assert!(body.contains("backend developer"), "{body}");
    assert!(
        body.contains("Project intelligence revision: 12."),
        "{body}"
    );
    assert!(body.contains("memory_read"), "{body}");
    assert!(!body.contains("Decision 1:"), "{body}");
    assert!(body.len() <= MAX_CONTINUATION_PROJECT_BYTES);
    assert!(
        body.len() * 3 < full_body.len(),
        "{} vs {}",
        body.len(),
        full_body.len()
    );
    let visible = registry.get("thread-1").expect("rule entries recorded");
    assert_eq!(visible.entry_for_alias("E2"), Some(("entry-02", 1)));
    assert_eq!(visible.entry_for_alias("E1"), None);
}

#[test]
fn rules_that_do_not_fit_keep_the_full_packet() {
    let long_rule = "Always explain every unit conversion step by step, with the source and the rounding used. ".repeat(4);
    let status = status_with((1..=12).map(|index| user_rule(index, &long_rule)).collect());
    assert!(continuation_project(&status).is_none());
    let unavailable = ProjectIntelligenceStatus::Unavailable {
        project_id: "project-1".to_string(),
    };
    assert!(continuation_project(&unavailable).is_none());
}

#[test]
fn a_revision_only_change_is_a_receipt_and_an_unchanged_window_renders_nothing() {
    let section = |revision| {
        let ProjectIntelligenceStatus::Available {
            project,
            last_refresh,
            root_blackboard,
        } = status_with(vec![user_rule(1, "Metric units only.")])
        else {
            unreachable!("fixture is available");
        };
        let RootBlackboardStatus::Available(mut root) = *root_blackboard else {
            unreachable!("fixture root is available");
        };
        root.projection.revision = revision;
        let status = ProjectIntelligenceStatus::Available {
            project,
            last_refresh,
            root_blackboard: Box::new(RootBlackboardStatus::Available(root)),
        };
        continuation_project_section(
            continuation_project(&status).expect("fits"),
            (VisibleRootRegistry::default(), "thread-1".to_string()),
        )
    };
    let first = section(12);
    let later = section(13);
    assert_eq!(
        rendered(&first, PreviousWorldStateSection::Known(first.snapshot())),
        None
    );
    let receipt =
        rendered(&later, PreviousWorldStateSection::Known(first.snapshot())).expect("receipt");
    assert!(receipt.contains("advanced from 12 to 13"), "{receipt}");
    assert!(!receipt.contains("Metric units only"), "{receipt}");
}

#[test]
fn continuation_run_packet_keeps_constraints_without_goal_or_obligation() {
    let section = run_continuation_section(run_status(WorkflowMode::Autonomous));
    let body = rendered(&section, PreviousWorldStateSection::Absent).expect("rendered");
    assert!(body.contains("Run ID: run-1"), "{body}");
    assert!(body.contains("expectedRevision: 3"), "{body}");
    assert!(body.contains("steering-1"), "{body}");
    assert!(body.contains("stateful_run_read"), "{body}");
    assert!(!body.contains("Fix the scaler"), "{body}");
    assert!(!body.contains("Eggs were scaled"), "{body}");
    assert!(!body.contains("Semantic checkpoint"), "{body}");
    assert_eq!(
        rendered(
            &section,
            PreviousWorldStateSection::Known(section.snapshot())
        ),
        None
    );
}
