use codex_extension_api::PreviousWorldStateSection;
use codex_project_intelligence::BlackboardKind;
use codex_project_intelligence::BlackboardProvenance;
use codex_project_intelligence::BlackboardProvenanceKind;
use codex_project_intelligence::RootBlackboardProjection;
use codex_stateful_runtime::WorkflowMode;
use pretty_assertions::assert_eq;

use super::Admission;
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
    let project = continuation_project(&status, Admission::Opening).expect("rules fit");
    let registry = VisibleRootRegistry::default();
    let section =
        continuation_project_section(project, None, (registry.clone(), "thread-1".to_string()));
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
    assert!(continuation_project(&status, Admission::Opening).is_none());
    let unavailable = ProjectIntelligenceStatus::Unavailable {
        project_id: "project-1".to_string(),
    };
    assert!(continuation_project(&unavailable, Admission::Opening).is_none());

    // A window that already opened as a continuation keeps its carrier and says what it
    // cannot show whole, instead of switching to the full packet.
    let truncated_rule = status_with(vec![user_rule(1, &"Keep every unit. ".repeat(300))]);
    let opened = continuation_project(&truncated_rule, Admission::Opened).expect("kept");
    let body = rendered(
        &continuation_project_section(
            opened,
            None,
            (VisibleRootRegistry::default(), "thread-1".to_string()),
        ),
        PreviousWorldStateSection::Absent,
    )
    .expect("rendered");
    assert!(
        body.contains("are not shown whole here; they still apply"),
        "{body}"
    );
    let unreadable = continuation_project(&unavailable, Admission::Opened).expect("kept");
    let body = rendered(
        &continuation_project_section(
            unreadable,
            None,
            (VisibleRootRegistry::default(), "thread-1".to_string()),
        ),
        PreviousWorldStateSection::Absent,
    )
    .expect("rendered");
    assert!(body.contains("could not be read for this step"), "{body}");
}

#[test]
fn a_changed_rule_is_one_exact_correction_not_a_second_carrier() {
    let section = |rules: Vec<codex_project_intelligence::BlackboardHit>| {
        continuation_project_section(
            continuation_project(&status_with(rules), Admission::Opened).expect("kept"),
            None,
            (VisibleRootRegistry::default(), "thread-1".to_string()),
        )
    };
    let before = section(vec![
        user_rule(1, "Metric units only."),
        user_rule(2, "Ask before editing recipes.json."),
    ]);
    let after = section(vec![
        user_rule(1, "Metric units only, grams for flour."),
        promoted_entry(2),
    ]);
    let fragment = after
        .render_diff(PreviousWorldStateSection::Known(before.snapshot()))
        .expect("correction");
    assert_eq!(
        fragment.markers(),
        ("<stateful_project_update>", "</stateful_project_update>")
    );
    let body = fragment.body();
    assert!(body.contains("grams for flour"), "{body}");
    assert!(body.contains("No longer shown or in force: E2."), "{body}");
    assert!(!body.contains("Project name"), "{body}");
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
            continuation_project(&status, Admission::Opening).expect("fits"),
            None,
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
    assert!(receipt.contains("revision is now 13"), "{receipt}");
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

#[test]
fn unreadable_memory_never_revokes_a_rule_and_an_unchanged_outage_is_silent() {
    let registry = || (VisibleRootRegistry::default(), "thread-1".to_string());
    let readable = continuation_project_section(
        continuation_project(
            &status_with(vec![user_rule(1, "Metric units only.")]),
            Admission::Opening,
        )
        .expect("fits"),
        None,
        registry(),
    );
    let unavailable = ProjectIntelligenceStatus::Unavailable {
        project_id: "project-1".to_string(),
    };
    let outage = continuation_project_section(
        continuation_project(&unavailable, Admission::Opened).expect("kept"),
        Some(readable.snapshot()),
        registry(),
    );
    let notice = outage
        .render_diff(PreviousWorldStateSection::Known(readable.snapshot()))
        .expect("notice")
        .body()
        .to_string();
    assert!(
        notice.contains("could not be read for this step"),
        "{notice}"
    );
    assert!(!notice.contains("No longer shown or in force"), "{notice}");
    assert!(notice.contains("do not complete the run yet"), "{notice}");
    // The persisted snapshot has no null fields, so the same outage renders nothing again.
    assert!(!outage.snapshot().to_string().contains("null"));
    assert_eq!(
        rendered(&outage, PreviousWorldStateSection::Known(outage.snapshot())),
        None
    );
    // Back to readable: the rule line is restated, nothing is revoked.
    let restored = readable
        .render_diff(PreviousWorldStateSection::Known(outage.snapshot()))
        .expect("restored")
        .body()
        .to_string();
    assert!(
        !restored.contains("No longer shown or in force"),
        "{restored}"
    );
}

#[test]
fn a_rule_retired_during_an_outage_is_revoked_when_memory_is_read_again() {
    let registry = || (VisibleRootRegistry::default(), "thread-1".to_string());
    let both = continuation_project_section(
        continuation_project(
            &status_with(vec![
                user_rule(1, "Metric units only."),
                user_rule(2, "Ask before editing recipes.json."),
            ]),
            Admission::Opening,
        )
        .expect("fits"),
        None,
        registry(),
    );
    let unavailable = ProjectIntelligenceStatus::Unavailable {
        project_id: "project-1".to_string(),
    };
    let outage = continuation_project_section(
        continuation_project(&unavailable, Admission::Opened).expect("kept"),
        Some(both.snapshot()),
        registry(),
    );
    // While memory is unreadable, the rules shown earlier keep their identity.
    let recovered = continuation_project_section(
        continuation_project(
            &status_with(vec![user_rule(1, "Metric units only.")]),
            Admission::Opened,
        )
        .expect("kept"),
        Some(outage.snapshot()),
        registry(),
    );
    let correction = recovered
        .render_diff(PreviousWorldStateSection::Known(outage.snapshot()))
        .expect("correction")
        .body()
        .to_string();
    assert!(
        correction.contains("No longer shown or in force: E2."),
        "{correction}"
    );
}
