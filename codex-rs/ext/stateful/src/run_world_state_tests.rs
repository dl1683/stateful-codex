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
            .contains("one stateful_run_update call carrying completionIdempotencyKey")
    );
    assert!(
        rendered
            .body()
            .contains("every intermediate update requires meaningful semantic change")
    );
    assert!(
        rendered
            .body()
            .contains("synthesizing, or comparing already-reviewed evidence")
    );
    assert!(rendered.body().contains("run result atomically"));
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
    });

    let rendered = section
        .render_diff(PreviousWorldStateSection::Absent)
        .expect("first contribution renders");
    assert!(rendered.body().contains("Stateful run state truncated"));
    assert!(rendered.body().len() <= super::MAX_BODY_BYTES);
}
