use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
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

use super::RunWorldStateStatus;
use super::run_world_state_section;

fn assert_fragment_bounded(fragment: &RenderedWorldStateFragment) {
    let (start, end) = fragment.markers();
    assert!(start.len() + fragment.body().len() + end.len() <= crate::limits::MAX_MODEL_ITEM_BYTES);
}

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
        acceptance: None,
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
            .contains("Semantic progress: while work remains, call obligation_update only when")
    );
    assert!(
        rendered
            .body()
            .contains("passing exactly expectedRevision, status completed, completionDisposition noReusableLearning, and result")
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
    assert_fragment_bounded(&rendered);
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
        acceptance: None,
    });

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("first contribution renders");
    assert!(
        rendered
            .body()
            .contains("Call stateful_run_read with section=\"goal\" and follow nextCursor")
    );
    // Collaborative runs carry no tool-call checkpoint nudges.
    assert!(!rendered.body().contains("Semantic checkpoint"));
    assert!(rendered.body().len() <= super::MAX_BODY_BYTES);
    assert_fragment_bounded(&rendered);
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
        acceptance: None,
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
    assert_fragment_bounded(&rendered);
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
            acceptance: None,
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
        acceptance: None,
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
            .contains("Semantic checkpoint: advisory due (process-local checkpoint 1)")
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
        acceptance: None,
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

#[test]
fn swapping_one_field_for_another_renders_the_full_packet() {
    let status = |mode: WorkflowMode, strategy: Option<&str>, revision: u64| {
        RunWorldStateStatus::Available {
            run: Box::new(StatefulRun {
                id: StatefulRunId::parse("run-1").expect("valid run id"),
                value: NewStatefulRun {
                    project_id: "project-1".to_string(),
                    thread_ids: vec!["thread-1".to_string()],
                    goal: "Find the decisive source constraint.".to_string(),
                    mode,
                    budget: RunBudget {
                        max_continuations: 12,
                        max_elapsed_seconds: 3_600,
                    },
                },
                status: StatefulRunStatus::Running,
                strategy: strategy.map(str::to_string),
                strategy_revision: 1,
                result: None,
                continuations_used: 0,
                revision,
                created_at_ms: 1,
                updated_at_ms: 2,
            }),
            obligation: None,
            steering: Vec::new(),
            steering_complete: true,
            checkpoint_due: None,
            acceptance: None,
        }
    };
    let previous =
        run_world_state_section(status(WorkflowMode::Autonomous, None, /*revision*/ 2));
    let current = run_world_state_section(status(
        WorkflowMode::Collaborative,
        Some("Compare the controlling sources."),
        /*revision*/ 3,
    ));

    let rendered = current
        .render_diff(PreviousWorldStateSection::Known(previous.snapshot()))
        .expect("a mode change must render");

    assert_eq!(rendered.markers(), ("<stateful_run>", "</stateful_run>"));
    assert!(!rendered.body().contains("Autonomous capacity"));
}

#[test]
fn multiline_obligation_items_stay_inside_the_obligation_block() {
    let section = run_world_state_section(run_with_obligation(
        "obligation-1",
        vec!["The permit was never transferred.\nIt is still in the seller's name.".to_string()],
    ));

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("first contribution renders");

    assert!(rendered.body().contains(
        "- Learned: The permit was never transferred. It is still in the seller's name."
    ));
}

#[test]
fn maximal_run_state_keeps_the_obligation_ahead_of_steering_within_bounds() {
    let run_id = StatefulRunId::parse(format!("run-{}", "r".repeat(500))).expect("valid run id");
    let project_id = format!("project-{}", "p".repeat(500));
    let item = "Learned clause detail \"quoted\" ".repeat(250);
    let steering = (0..5)
        .map(|index| StatefulSteering {
            id: SteeringId::parse(format!("steering-{index}-{}", "s".repeat(490)))
                .expect("valid steering id"),
            value: NewSteeringInstruction {
                project_id: project_id.clone(),
                run_id: run_id.clone(),
                input: "Prefer the signed schedule. ".repeat(600),
                affected_obligation_ids: Vec::new(),
            },
            status: SteeringStatus::Submitted,
            resulting_strategy_revision: None,
            reason: None,
            revision: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
        })
        .collect::<Vec<_>>();
    let section = run_world_state_section(RunWorldStateStatus::Available {
        run: Box::new(StatefulRun {
            id: run_id.clone(),
            value: NewStatefulRun {
                project_id: project_id.clone(),
                thread_ids: vec!["thread-1".to_string()],
                goal: "Review the agreement. ".repeat(700),
                mode: WorkflowMode::Autonomous,
                budget: RunBudget {
                    max_continuations: 12,
                    max_elapsed_seconds: 3_600,
                },
            },
            status: StatefulRunStatus::Running,
            strategy: Some("Compare the schedules. ".repeat(700)),
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
                project_id,
                run_id,
                packet: ObligationPacket {
                    learning: vec![item.clone(); 32],
                    next: vec![item; 32],
                    ..Default::default()
                },
                provenance_source_id: "turn-1".to_string(),
            },
            sequence: 1,
            revision: 1,
            created_at_ms: 2,
        })),
        steering,
        steering_complete: true,
        checkpoint_due: Some(3),
        acceptance: None,
    });

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("first contribution renders");
    let body = rendered.body();
    let (start, end) = rendered.markers();
    let obligation_at = body
        .find("Current semantic obligation:")
        .expect("obligation heading");
    let steering_at = body
        .find("Unresolved user steering:")
        .expect("steering heading");

    assert!(start.len() + body.len() + end.len() <= 9_000);
    assert!(obligation_at < steering_at);
    assert!(steering_at - obligation_at >= 2 * 1024);
    assert!(body.contains("Obligation shortened:"));
    assert!(body.contains("section=\"obligation\""));
    assert!(body.contains("This bounded view omitted or shortened unresolved steering."));
    assert!(body.contains("Semantic checkpoint: overdue (process-local checkpoint 3)"));
}

#[test]
fn unanswered_checkpoint_escalates_through_a_delta() {
    let status = |checkpoint_due: Option<u64>| {
        let mut status = run_with_obligation("obligation-1", vec!["Found the permit.".to_string()]);
        let RunWorldStateStatus::Available {
            run,
            checkpoint_due: due,
            ..
        } = &mut status
        else {
            unreachable!("fixture is available");
        };
        run.value.mode = WorkflowMode::Autonomous;
        *due = checkpoint_due;
        status
    };
    let advisory = run_world_state_section(status(Some(1)));
    let overdue = run_world_state_section(status(Some(2)));

    let rendered = overdue
        .render_diff(PreviousWorldStateSection::Known(advisory.snapshot()))
        .expect("escalation must render");

    assert_eq!(
        rendered.markers(),
        ("<stateful_run_update>", "</stateful_run_update>")
    );
    assert!(rendered.body().contains("Call obligation_update now"));
}

#[test]
fn collaborative_checkpoint_epochs_do_not_emit_updates() {
    let status = |checkpoint_due: Option<u64>| {
        let mut status = run_with_obligation("obligation-1", vec!["Found the permit.".to_string()]);
        let RunWorldStateStatus::Available {
            checkpoint_due: due,
            ..
        } = &mut status
        else {
            unreachable!("fixture is available");
        };
        *due = checkpoint_due;
        status
    };
    let before = run_world_state_section(status(None));
    let after = run_world_state_section(status(Some(2)));

    assert_eq!(
        after.render_diff(PreviousWorldStateSection::Known(before.snapshot())),
        None
    );
}

/// The exact snapshot the previous build (4e9426f0bd) recorded for this fixture, whose
/// Collaborative policy told the model to complete the run at the end of every turn.
const PRE_POLICY_CHANGE_SNAPSHOT: &str = r#"{"fieldKeys":["This is the durable Stateful run selected by the user. Its mode, goal, and binding constraints are explicit; do not infer replacements.","Project ID","Run ID","Run revision","Run-update precondition","Strategy revision","Mode","Status","Mode obligation","Stateful write tools (blackboard_record_batch, blackboard_update_batch, blackboard_relate, obligation_update, stateful_run_update, steering_reconcile) are direct function tools and are not callable inside exec. Complete once, as the final Stateful mutation","Goal"],"fields":["fb0e6575a670c8d1e3b5915d842897f1","ec2317830b07d7c9f282d368c025145f","94940dab6976735a579722d0306fc3f8","a01b52cf6457d0da0e50f4e4c2f0bbcc","02468ada597b916221638c598a1533bd","83f186cb17ba59a2aed6015da903c4ce","5c70a64017080ed126527e4619f98258","a1fab10431c833217fdedb0b32fc617b","8c18f40451efbec274033a1069f188dd","201db515dbafce96c4fbf460d9d63eb8","8ccf5da72cadcc3160c2520281b8a8ac"],"fingerprint":"4923394d9486cab5ea1b83b63840831ca0f2c3026a17bff7f0360a387d3bec27","obligation":"1d180498f7db3a82eb41e171b48fa7fe","runId":"run-1","steering":"c2acfe2f98a773bb34065067994ca31e"}"#;

#[test]
fn changed_collaborative_policy_replaces_a_retained_packet() {
    let section = run_world_state_section(run_with_obligation(
        "obligation-1",
        vec!["Found the permit.".to_string()],
    ));
    let previous: serde_json::Value =
        serde_json::from_str(PRE_POLICY_CHANGE_SNAPSHOT).expect("recorded snapshot parses");

    let rendered = section
        .render_diff(PreviousWorldStateSection::Known(&previous))
        .expect("a changed policy renders");
    assert_eq!(rendered.markers(), ("<stateful_run>", "</stateful_run>"));
    assert!(
        rendered
            .body()
            .contains("stays open across the user's turns")
    );
}
