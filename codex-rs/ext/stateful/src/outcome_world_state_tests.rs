use codex_extension_api::PreviousWorldStateSection;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunOutcome;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;
use pretty_assertions::assert_eq;

use super::MAX_BODY_BYTES;
use super::ProjectOutcomesStatus;
use super::project_outcomes_world_state_section;

fn outcome(id: &str, goal: &str, result: &str, learning: &str) -> StatefulRunOutcome {
    let run_id = StatefulRunId::parse(id).expect("valid run ID");
    StatefulRunOutcome {
        run: StatefulRun {
            id: run_id.clone(),
            value: NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: goal.to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Completed,
            strategy: None,
            strategy_revision: 0,
            result: Some(result.to_string()),
            continuations_used: 1,
            revision: 3,
            created_at_ms: 1,
            updated_at_ms: 2,
        },
        final_obligation: Some(StatefulObligation {
            id: format!("{id}-final"),
            value: NewObligation {
                project_id: "project-1".to_string(),
                run_id,
                packet: ObligationPacket {
                    learning: vec![learning.to_string()],
                    implication: vec!["Use the prior conclusion in later work.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "completion-turn".to_string(),
            },
            sequence: 1,
            revision: 1,
            created_at_ms: 2,
        }),
    }
}

fn available(outcomes: Vec<StatefulRunOutcome>) -> ProjectOutcomesStatus {
    ProjectOutcomesStatus::Available {
        project_id: "project-1".to_string(),
        outcomes,
        has_more: false,
    }
}

#[test]
fn renders_bounded_completed_outcomes_for_project_continuity() {
    let section = project_outcomes_world_state_section(available(vec![outcome(
        "run-1",
        "Determine the controlling threshold.",
        "The controlling threshold is six.",
        "The current policy replaced the prior threshold.",
    )]));

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("recent outcomes should render");

    assert_eq!(
        rendered.markers(),
        (
            "<stateful_project_outcomes>",
            "</stateful_project_outcomes>"
        )
    );
    assert!(rendered.body().contains("Project ID: project-1"));
    assert!(
        rendered
            .body()
            .contains("The controlling threshold is six.")
    );
    assert!(
        rendered
            .body()
            .contains("The current policy replaced the prior threshold.")
    );
    assert!(rendered.body().len() <= MAX_BODY_BYTES);
}

#[test]
fn known_snapshot_renders_only_new_completed_outcomes() {
    let prior = outcome(
        "run-1",
        "Resolve the first question.",
        "The first result is durable.",
        "The first conclusion is known.",
    );
    let previous = project_outcomes_world_state_section(available(vec![prior.clone()]));
    let current = project_outcomes_world_state_section(available(vec![
        outcome(
            "run-2",
            "Resolve the second question.",
            "The second result is durable.",
            "The second conclusion is new.",
        ),
        prior,
    ]));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("new outcome should render as a delta");

    assert_eq!(
        rendered.markers(),
        (
            "<stateful_project_outcomes_update>",
            "</stateful_project_outcomes_update>"
        )
    );
    assert!(rendered.body().contains("The second result is durable."));
    assert!(!rendered.body().contains("The first result is durable."));
}

#[test]
fn long_outcomes_are_truncated_on_utf8_boundaries() {
    let section = project_outcomes_world_state_section(available(vec![outcome(
        "run-long",
        &"🙂".repeat(2_000),
        &"résultat".repeat(2_000),
        &"学び".repeat(2_000),
    )]));
    let rendered = section
        .render_diff(PreviousWorldStateSection::Unknown)
        .expect("unknown state should render the current bounded outcomes");

    assert!(rendered.body().len() <= MAX_BODY_BYTES);
    assert!(std::str::from_utf8(rendered.body().as_bytes()).is_ok());
}
