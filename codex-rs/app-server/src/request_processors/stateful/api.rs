use codex_app_server_protocol::StatefulObligation as ApiObligation;
use codex_app_server_protocol::StatefulObligationPacket as ApiObligationPacket;
use codex_app_server_protocol::StatefulRun as ApiRun;
use codex_app_server_protocol::StatefulRunBudget as ApiRunBudget;
use codex_app_server_protocol::StatefulRunStatus as ApiRunStatus;
use codex_app_server_protocol::StatefulSteering as ApiSteering;
use codex_app_server_protocol::StatefulSteeringStatus as ApiSteeringStatus;
use codex_app_server_protocol::StatefulWorkflowMode as ApiWorkflowMode;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::SteeringStatus;
use codex_stateful_runtime::WorkflowMode;

pub(super) fn workflow_mode(value: ApiWorkflowMode) -> WorkflowMode {
    match value {
        ApiWorkflowMode::Autonomous => WorkflowMode::Autonomous,
        ApiWorkflowMode::Collaborative => WorkflowMode::Collaborative,
        ApiWorkflowMode::Socratic => WorkflowMode::Socratic,
    }
}

pub(super) fn api_run(value: StatefulRun) -> ApiRun {
    ApiRun {
        id: value.id.to_string(),
        project_id: value.value.project_id,
        thread_ids: value.value.thread_ids,
        goal: value.value.goal,
        mode: match value.value.mode {
            WorkflowMode::Autonomous => ApiWorkflowMode::Autonomous,
            WorkflowMode::Collaborative => ApiWorkflowMode::Collaborative,
            WorkflowMode::Socratic => ApiWorkflowMode::Socratic,
        },
        budget: ApiRunBudget {
            max_continuations: value.value.budget.max_continuations,
            max_elapsed_seconds: value.value.budget.max_elapsed_seconds,
        },
        continuations_used: value.continuations_used,
        status: match value.status {
            StatefulRunStatus::Pending => ApiRunStatus::Pending,
            StatefulRunStatus::Running => ApiRunStatus::Running,
            StatefulRunStatus::Paused => ApiRunStatus::Paused,
            StatefulRunStatus::Completed => ApiRunStatus::Completed,
            StatefulRunStatus::Cancelled => ApiRunStatus::Cancelled,
            StatefulRunStatus::Blocked => ApiRunStatus::Blocked,
            StatefulRunStatus::Failed => ApiRunStatus::Failed,
        },
        strategy: value.strategy,
        strategy_revision: value.strategy_revision,
        result: value.result,
        revision: value.revision,
        created_at: value.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: value.updated_at_ms.div_euclid(/*rhs*/ 1000),
    }
}

pub(super) fn api_obligation(value: StatefulObligation) -> ApiObligation {
    let packet = value.value.packet;
    ApiObligation {
        id: value.id,
        project_id: value.value.project_id,
        run_id: value.value.run_id.to_string(),
        packet: ApiObligationPacket {
            examined: packet.examined,
            rationale: packet.rationale,
            learning: packet.learning,
            implication: packet.implication,
            strategy: packet.strategy,
            changed: packet.changed,
            next: packet.next,
            uncertainty: packet.uncertainty,
            blockers: packet.blockers,
            requested_judgment: packet.requested_judgment,
        },
        provenance_source_id: value.value.provenance_source_id,
        sequence: value.sequence,
        revision: value.revision,
        created_at: value.created_at_ms.div_euclid(/*rhs*/ 1000),
    }
}

pub(super) fn api_steering(value: StatefulSteering) -> ApiSteering {
    ApiSteering {
        id: value.id.to_string(),
        project_id: value.value.project_id,
        run_id: value.value.run_id.to_string(),
        input: value.value.input,
        affected_obligation_ids: value.value.affected_obligation_ids,
        status: match value.status {
            SteeringStatus::Submitted => ApiSteeringStatus::Submitted,
            SteeringStatus::Acknowledged => ApiSteeringStatus::Acknowledged,
            SteeringStatus::Applied => ApiSteeringStatus::Applied,
            SteeringStatus::Rejected => ApiSteeringStatus::Rejected,
        },
        resulting_strategy_revision: value.resulting_strategy_revision,
        reason: value.reason,
        revision: value.revision,
        created_at: value.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: value.updated_at_ms.div_euclid(/*rhs*/ 1000),
    }
}
