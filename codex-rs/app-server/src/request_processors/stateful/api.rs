use codex_app_server_protocol::StatefulAttributionCounters as ApiAttributionCounters;
use codex_app_server_protocol::StatefulMeasurementSummary as ApiMeasurementSummary;
use codex_app_server_protocol::StatefulObligation as ApiObligation;
use codex_app_server_protocol::StatefulObligationPacket as ApiObligationPacket;
use codex_app_server_protocol::StatefulRun as ApiRun;
use codex_app_server_protocol::StatefulRunBudget as ApiRunBudget;
use codex_app_server_protocol::StatefulRunStatus as ApiRunStatus;
use codex_app_server_protocol::StatefulSteering as ApiSteering;
use codex_app_server_protocol::StatefulSteeringStatus as ApiSteeringStatus;
use codex_app_server_protocol::StatefulTurnMeasurement as ApiTurnMeasurement;
use codex_app_server_protocol::StatefulTurnStatus as ApiTurnStatus;
use codex_app_server_protocol::StatefulWorkflowMode as ApiWorkflowMode;
use codex_app_server_protocol::TokenUsageBreakdown;
use codex_stateful_runtime::StatefulMeasurementSummary;
use codex_stateful_runtime::StatefulObligation;
use codex_stateful_runtime::StatefulRun;
use codex_stateful_runtime::StatefulRunStatus;
use codex_stateful_runtime::StatefulSteering;
use codex_stateful_runtime::StatefulTurnMeasurement;
use codex_stateful_runtime::StatefulTurnStatus;
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

pub(super) fn api_turn_measurement(value: StatefulTurnMeasurement) -> ApiTurnMeasurement {
    ApiTurnMeasurement {
        run_id: value.value.run_id.to_string(),
        project_id: value.value.project_id,
        thread_id: value.value.thread_id,
        turn_id: value.value.turn_id,
        status: match value.value.status {
            StatefulTurnStatus::Completed => ApiTurnStatus::Completed,
            StatefulTurnStatus::Failed => ApiTurnStatus::Failed,
            StatefulTurnStatus::Aborted => ApiTurnStatus::Aborted,
        },
        duration_ms: value.value.duration_ms,
        counters: api_attribution_counters(value.value.attribution_counters),
        trajectory: value.trajectory.map(api_trajectory),
        token_usage: value.token_usage.map(api_token_usage),
        completed_at: value
            .completed_at_ms
            .map(|timestamp| timestamp.div_euclid(/*rhs*/ 1_000)),
        created_at: value.created_at_ms.div_euclid(/*rhs*/ 1_000),
        updated_at: value.updated_at_ms.div_euclid(/*rhs*/ 1_000),
    }
}

pub(super) fn api_measurement_summary(value: StatefulMeasurementSummary) -> ApiMeasurementSummary {
    ApiMeasurementSummary {
        project_id: value.project_id,
        measurement_count: value.measurement_count,
        run_count: value.run_count,
        terminal_measurement_count: value.terminal_measurement_count,
        completed_turns: value.completed_turns,
        failed_turns: value.failed_turns,
        aborted_turns: value.aborted_turns,
        turns_with_token_usage: value.turns_with_token_usage,
        duration_ms: value.duration_ms,
        counters: api_attribution_counters(value.attribution_counters),
        trajectory: value.trajectory.map(api_trajectory),
        token_usage: value.token_usage.map(api_token_usage),
        oldest_created_at: value
            .oldest_created_at_ms
            .map(|timestamp| timestamp.div_euclid(/*rhs*/ 1_000)),
        newest_created_at: value
            .newest_created_at_ms
            .map(|timestamp| timestamp.div_euclid(/*rhs*/ 1_000)),
        has_more: value.has_more,
    }
}

fn api_attribution_counters(
    counters: codex_stateful_runtime::StatefulAttributionCounters,
) -> ApiAttributionCounters {
    ApiAttributionCounters {
        world_state_samples: counters.world_state_samples,
        root_entries_loaded: counters.root_entries_loaded,
        root_evidence_routes_checked: counters.root_evidence_routes_checked,
        root_evidence_routes_current: counters.root_evidence_routes_current,
        root_evidence_routes_stale: counters.root_evidence_routes_stale,
        root_evidence_routes_unavailable: counters.root_evidence_routes_unavailable,
        root_evidence_routes_unchecked: counters.root_evidence_routes_unchecked,
        root_unique_sources_observed: counters.root_unique_sources_observed,
        root_source_bytes_hashed: counters.root_source_bytes_hashed,
        stateful_tool_calls: counters.stateful_tool_calls,
        failed_stateful_tool_calls: counters.failed_stateful_tool_calls,
        knowledge_query_calls: counters.knowledge_query_calls,
        route_query_calls: counters.route_query_calls,
        evidence_read_calls: counters.evidence_read_calls,
        steering_query_calls: counters.steering_query_calls,
        conversation_read_calls: counters.conversation_read_calls,
        blackboard_write_calls: counters.blackboard_write_calls,
        context_refresh_calls: counters.context_refresh_calls,
        obligation_write_calls: counters.obligation_write_calls,
        run_update_calls: counters.run_update_calls,
        steering_write_calls: counters.steering_write_calls,
        material_findings_reused: counters.material_findings_reused,
    }
}

fn api_trajectory(
    trajectory: codex_stateful_runtime::TurnTrajectory,
) -> codex_app_server_protocol::TurnTrajectory {
    codex_app_server_protocol::TurnTrajectory {
        completed_model_responses: trajectory.completed_model_responses,
        compactions: trajectory.compactions,
        model_tool_calls: trajectory.model_tool_calls,
        model_shell_tool_calls: trajectory.model_shell_tool_calls,
        model_function_tool_calls: trajectory.model_function_tool_calls,
        model_custom_tool_calls: trajectory.model_custom_tool_calls,
        model_tool_search_calls: trajectory.model_tool_search_calls,
        model_web_search_calls: trajectory.model_web_search_calls,
        model_image_generation_calls: trajectory.model_image_generation_calls,
        tool_output_bytes: trajectory.tool_output_bytes,
    }
}

fn api_token_usage(usage: codex_stateful_runtime::StatefulTokenUsage) -> TokenUsageBreakdown {
    TokenUsageBreakdown {
        total_tokens: usage.total_tokens,
        input_tokens: usage.input_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        cache_write_input_tokens: usage.cache_write_input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_output_tokens: usage.reasoning_output_tokens,
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
