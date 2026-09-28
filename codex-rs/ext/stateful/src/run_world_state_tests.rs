use codex_extension_api::PreviousWorldStateSection;
use codex_stateful_runtime::NewObligation;
use codex_stateful_runtime::NewStatefulRun;
use codex_stateful_runtime::ObligationPacket;
use codex_stateful_runtime::RunBudget;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunId;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::WorkflowMode;

use super::RunWorldStateStatus;
use super::run_world_state_section;

#[test]
fn run_world_state_is_semantic_bounded_and_stable() {
    let run_id = StatefulRunId::parse("run-1").expect("valid run id");
    let section = run_world_state_section(RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: run_id.clone(),
            value: NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Find the decisive source constraint.".to_string(),
                mode: WorkflowMode::Socratic,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Pending,
            strategy: Some("Resolve the material assumptions first.".to_string()),
            strategy_revision: 1,
            result: None,
            continuations_used: 0,
            revision: 2,
            created_at_ms: 1,
            updated_at_ms: 2,
        }),
        obligation: Some(Box::new(StatefulObligation {
            id: "obligation-1".to_string(),
            value: NewObligation {
                project_id: "project-1".to_string(),
                run_id,
                packet: ObligationPacket {
                    learning: vec!["One unresolved assumption controls execution.".to_string()],
                    next: vec!["Ask the user to resolve it.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "turn-1".to_string(),
            },
            sequence: 1,
            revision: 1,
            created_at_ms: 2,
        })),
        steering: Vec::new(),
        steering_complete: true,
        checkpoint_due: None,
    });
    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("first contribution renders");
    assert!(rendered.body().contains("Mode: Socratic"));
    assert!(rendered.body().contains("Run revision: 2"));
    assert!(
        rendered
            .body()
            .contains("pass expectedRevision: 2 to stateful_run_update")
    );
    assert!(
        rendered
            .body()
            .contains("never substitute the separate project intelligence revision")
    );
    assert!(rendered.body().contains("Strategy revision: 1"));
    assert!(rendered.body().contains("Do not invoke execution tools"));
    assert!(rendered.body().contains("Unresolved user steering: none"));
    assert!(
        rendered
            .body()
            .contains("Semantic progress: call obligation_update whenever learning")
    );
    assert!(
        rendered
            .body()
            .contains("completionDisposition noReusableLearning with only the result")
    );
    assert!(
        rendered
            .body()
            .contains("Completion must be the final Stateful mutation.")
    );
    assert!(
        rendered
            .body()
            .contains("Learned: One unresolved assumption")
    );
    assert!(rendered.body().len() <= super::MAX_BODY_BYTES);
    assert!(
        section
            .render_diff(PreviousWorldStateSection::Known(section.snapshot()))
            .is_none()
    );
}

#[test]
fn run_world_state_discloses_omitted_detail() {
    let section = run_world_state_section(RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: StatefulRunId::parse("run-large").expect("valid run id"),
            value: NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "x".repeat(super::MAX_BODY_BYTES * 2),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Running,
            strategy: None,
            strategy_revision: 0,
            result: None,
            continuations_used: 0,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
        }),
        obligation: None,
        steering: Vec::new(),
        steering_complete: true,
        checkpoint_due: None,
    });

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("first contribution renders");
    assert!(
        rendered
            .body()
            .contains("[goal shortened; the full goal is in the run record]")
    );
    assert!(rendered.body().contains("Semantic checkpoint: current."));
    assert!(rendered.body().len() <= super::MAX_BODY_BYTES);
}

#[test]
fn obligation_change_replaces_only_the_obligation_block() {
    let run_id = StatefulRunId::parse("run-1").expect("valid run id");
    let status = |learning: &str, revision: u64| RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: run_id.clone(),
            value: NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Find the decisive source constraint.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Running,
            strategy: Some("Compare the controlling sources.".to_string()),
            strategy_revision: 1,
            result: None,
            continuations_used: 0,
            revision: 2,
            created_at_ms: 1,
            updated_at_ms: 2,
        }),
        obligation: Some(Box::new(StatefulObligation {
            id: format!("obligation-{revision}"),
            value: NewObligation {
                project_id: "project-1".to_string(),
                run_id: run_id.clone(),
                packet: ObligationPacket {
                    learning: vec![learning.to_string()],
                    next: vec!["Check the lease consent.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "turn-1".to_string(),
            },
            sequence: revision,
            revision: 1,
            created_at_ms: 2,
        })),
        steering: Vec::new(),
        steering_complete: true,
        checkpoint_due: None,
    };
    let previous = run_world_state_section(status("The permit was never transferred.", 1));
    let current = run_world_state_section(status("The landlord consent is also missing.", 2));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("obligation change must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_run_update>", "</stateful_run_update>")
    );
    let body = rendered.body();
    assert!(body.contains("Run ID: run-1"));
    assert!(body.contains("Semantic obligation (replaces the previous obligation entirely):"));
    assert!(body.contains("- Learned: The landlord consent is also missing."));
    assert!(body.contains("- Next: Check the lease consent."));
    assert!(!body.contains("Goal:"));
    assert!(!body.contains("Semantic progress:"));
}

#[test]
fn different_run_renders_the_full_packet() {
    let section = |id: &str| {
        run_world_state_section(RunWorldStateStatus::Available {
            run: Box::new(StatefulRun {
                id: StatefulRunId::parse(id).expect("valid run id"),
                value: NewStatefulRun {
                    project_id: "project-1".to_string(),
                    thread_ids: vec!["thread-1".to_string()],
                    goal: "Answer the question.".to_string(),
                    mode: WorkflowMode::Collaborative,
                    budget: RunBudget {
                        max_continuations: 12,
                        max_elapsed_seconds: 3_600,
                    },
                },
                status: StatefulRunStatus::Running,
                strategy: None,
                strategy_revision: 0,
                result: None,
                continuations_used: 0,
                revision: 1,
                created_at_ms: 1,
                updated_at_ms: 1,
            }),
            obligation: None,
            steering: Vec::new(),
            steering_complete: true,
            checkpoint_due: None,
        })
    };
    let previous = section("run-1");
    let current = section("run-2");

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("a new run must render");

    assert_eq!(rendered.markers(), ("<stateful_run>", "</stateful_run>"));
}

#[test]
fn due_checkpoint_renders_one_line_that_changes_only_per_epoch() {
    let status = |checkpoint_due: Option<u64>| RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: StatefulRunId::parse("run-1").expect("valid run id"),
            value: NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Document the harness.".to_string(),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Running,
            strategy: None,
            strategy_revision: 0,
            result: None,
            continuations_used: 0,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
        }),
        obligation: None,
        steering: Vec::new(),
        steering_complete: true,
        checkpoint_due,
    };
    let current = run_world_state_section(status(None));
    let due = run_world_state_section(status(Some(1)));

    let rendered = due
        .render_diff(PreviousWorldStateSection::Known(current.snapshot()))
        .expect("a due checkpoint must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_run_update>", "</stateful_run_update>")
    );
    assert!(
        rendered
            .body()
            .contains("Semantic checkpoint due (checkpoint 1)")
    );
    assert!(!rendered.body().contains("Goal:"));
    assert!(
        run_world_state_section(status(Some(1)))
            .render_diff(PreviousWorldStateSection::Known(due.snapshot()))
            .is_none()
    );
}

fn run_with_obligation(obligation_id: &str, learning: Vec<String>) -> RunWorldStateStatus {
    let run_id = StatefulRunId::parse("run-1").expect("valid run id");
    RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: run_id.clone(),
            value: NewStatefulRun {
                project_id: "project-1".to_string(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Find the decisive source constraint.".to_string(),
                mode: WorkflowMode::Collaborative,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Running,
            strategy: None,
            strategy_revision: 0,
            result: None,
            continuations_used: 0,
            revision: 2,
            created_at_ms: 1,
            updated_at_ms: 2,
        }),
        obligation: Some(Box::new(StatefulObligation {
            id: obligation_id.to_string(),
            value: NewObligation {
                project_id: "project-1".to_string(),
                run_id,
                packet: ObligationPacket {
                    learning,
                    next: vec!["Check the lease consent.".to_string()],
                    ..Default::default()
                },
                provenance_source_id: "turn-1".to_string(),
            },
            sequence: 1,
            revision: 1,
            created_at_ms: 2,
        })),
        steering: Vec::new(),
        steering_complete: true,
        checkpoint_due: None,
    }
}

#[test]
fn deletion_only_obligation_change_still_replaces_the_block() {
    let previous = run_world_state_section(run_with_obligation(
        "obligation-1",
        vec![
            "The permit was never transferred.".to_string(),
            "The landlord consent is missing.".to_string(),
        ],
    ));
    let current = run_world_state_section(run_with_obligation(
        "obligation-2",
        vec!["The permit was never transferred.".to_string()],
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("a removed obligation item must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_run_update>", "</stateful_run_update>")
    );
    assert!(
        rendered
            .body()
            .contains("Semantic obligation (replaces the previous obligation entirely):")
    );
    assert!(!rendered.body().contains("The landlord consent is missing."));
}

#[test]
fn unknown_previous_run_state_renders_the_full_packet() {
    let section = run_world_state_section(run_with_obligation(
        "obligation-1",
        vec!["The permit was never transferred.".to_string()],
    ));

    let rendered = section
        .render_diff(PreviousWorldStateSection::Unknown)
        .expect("unknown retained state must render");

    assert_eq!(rendered.markers(), ("<stateful_run>", "</stateful_run>"));
}
