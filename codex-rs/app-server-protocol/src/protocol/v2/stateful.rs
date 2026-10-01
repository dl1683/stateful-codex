use crate::JsonSchema;
use crate::TS;
use codex_experimental_api_macros::ExperimentalApi;
use serde::Deserialize;
use serde::Serialize;

use super::TokenUsageBreakdown;
use super::TurnTrajectory;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulWorkflowMode {
    Autonomous,
    Collaborative,
    Socratic,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulRunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Cancelled,
    Blocked,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunBudget {
    pub max_continuations: u32,
    pub max_elapsed_seconds: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRun {
    pub id: String,
    pub project_id: String,
    pub thread_ids: Vec<String>,
    pub goal: String,
    pub mode: StatefulWorkflowMode,
    pub budget: StatefulRunBudget,
    pub continuations_used: u32,
    pub status: StatefulRunStatus,
    pub strategy: Option<String>,
    #[ts(type = "number")]
    pub strategy_revision: u64,
    pub result: Option<String>,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunStartParams {
    pub project_id: String,
    pub thread_id: String,
    pub goal: String,
    pub mode: StatefulWorkflowMode,
    pub budget: StatefulRunBudget,
    pub idempotency_key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunStartResponse {
    pub run: StatefulRun,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunReadParams {
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
    #[ts(optional = nullable)]
    pub thread_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunReadResponse {
    pub run: Option<StatefulRun>,
    pub recovery: Option<StatefulRunRecovery>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunRecovery {
    pub lease_expires_at: Option<i64>,
    pub previous_turn_id: Option<String>,
    pub last_continuation_claimed_at: Option<i64>,
}

macro_rules! run_control_params {
    ($name:ident) => {
        #[derive(
            Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi,
        )]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "v2/")]
        pub struct $name {
            pub run_id: String,
            #[ts(type = "number")]
            pub expected_revision: u64,
        }
    };
}

run_control_params!(StatefulRunPauseParams);
run_control_params!(StatefulRunResumeParams);
run_control_params!(StatefulRunCancelParams);

macro_rules! run_control_response {
    ($name:ident) => {
        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "v2/")]
        pub struct $name {
            pub run: StatefulRun,
        }
    };
}

run_control_response!(StatefulRunPauseResponse);
run_control_response!(StatefulRunResumeResponse);
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunCancelResponse {
    pub run: StatefulRun,
    /// Turns of this run that were active in this app-server and were interrupted.
    /// A run turn executing in another process is not interrupted; it stops being
    /// continued because the run is no longer running.
    pub interrupted_turn_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunSetModeParams {
    pub run_id: String,
    #[ts(type = "number")]
    pub expected_revision: u64,
    pub mode: StatefulWorkflowMode,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunSetModeResponse {
    pub run: StatefulRun,
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema, TS, ExperimentalApi,
)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulObligationPacket {
    #[serde(default)]
    pub examined: Vec<String>,
    #[serde(default)]
    pub rationale: Vec<String>,
    #[serde(default)]
    pub learning: Vec<String>,
    #[serde(default)]
    pub implication: Vec<String>,
    #[serde(default)]
    pub strategy: Vec<String>,
    #[serde(default)]
    pub changed: Vec<String>,
    #[serde(default)]
    pub next: Vec<String>,
    #[serde(default)]
    pub uncertainty: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub requested_judgment: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulObligation {
    pub id: String,
    pub project_id: String,
    pub run_id: String,
    pub packet: StatefulObligationPacket,
    pub provenance_source_id: String,
    #[ts(type = "number")]
    pub sequence: u64,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ObligationListParams {
    pub run_id: String,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ObligationListResponse {
    pub data: Vec<StatefulObligation>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulSteeringStatus {
    Submitted,
    Acknowledged,
    Applied,
    Rejected,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulSteering {
    pub id: String,
    pub project_id: String,
    pub run_id: String,
    pub input: String,
    pub affected_obligation_ids: Vec<String>,
    pub status: StatefulSteeringStatus,
    #[ts(type = "number | null")]
    pub resulting_strategy_revision: Option<u64>,
    pub reason: Option<String>,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SteeringSubmitParams {
    pub run_id: String,
    pub input: String,
    pub affected_obligation_ids: Vec<String>,
    pub idempotency_key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SteeringSubmitResponse {
    pub steering: StatefulSteering,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SteeringListParams {
    pub run_id: String,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SteeringListResponse {
    pub data: Vec<StatefulSteering>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulRunUpdatedNotification {
    pub project_id: String,
    pub run_id: String,
    #[ts(type = "number")]
    pub revision: u64,
    pub cursor: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ObligationUpdatedNotification {
    pub project_id: String,
    pub run_id: String,
    pub obligation_id: String,
    #[ts(type = "number")]
    pub revision: u64,
    pub cursor: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SteeringUpdatedNotification {
    pub project_id: String,
    pub run_id: String,
    pub steering_id: String,
    #[ts(type = "number")]
    pub revision: u64,
    pub cursor: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulAttributionStatus {
    Completed,
    Failed,
    Aborted,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulAttributionCounters {
    #[ts(type = "number")]
    pub world_state_samples: u64,
    #[ts(type = "number")]
    pub root_entries_loaded: u64,
    #[ts(type = "number")]
    pub root_evidence_routes_checked: u64,
    #[ts(type = "number")]
    pub root_evidence_routes_current: u64,
    #[ts(type = "number")]
    pub root_evidence_routes_stale: u64,
    #[ts(type = "number")]
    pub root_evidence_routes_unavailable: u64,
    #[ts(type = "number")]
    pub root_evidence_routes_unchecked: u64,
    #[ts(type = "number")]
    pub root_unique_sources_observed: u64,
    #[ts(type = "number")]
    pub root_source_bytes_hashed: u64,
    #[ts(type = "number")]
    pub stateful_tool_calls: u64,
    #[ts(type = "number")]
    pub failed_stateful_tool_calls: u64,
    #[ts(type = "number")]
    pub knowledge_query_calls: u64,
    #[ts(type = "number")]
    pub route_query_calls: u64,
    #[ts(type = "number")]
    pub evidence_read_calls: u64,
    #[ts(type = "number")]
    pub steering_query_calls: u64,
    #[ts(type = "number")]
    pub blackboard_write_calls: u64,
    #[ts(type = "number")]
    pub context_refresh_calls: u64,
    #[ts(type = "number")]
    pub obligation_write_calls: u64,
    #[ts(type = "number")]
    pub run_update_calls: u64,
    #[ts(type = "number")]
    pub steering_write_calls: u64,
    #[ts(type = "number")]
    pub material_findings_reused: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub enum StatefulTurnStatus {
    Completed,
    Failed,
    Aborted,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulTurnMeasurement {
    pub run_id: String,
    pub project_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub status: StatefulTurnStatus,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub counters: StatefulAttributionCounters,
    pub trajectory: Option<TurnTrajectory>,
    /// Provider-reported usage accumulated across completed responses in this
    /// turn. `None` means no response included usage data.
    pub token_usage: Option<TokenUsageBreakdown>,
    #[ts(type = "number | null")]
    pub completed_at: Option<i64>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMeasurementListParams {
    pub project_id: String,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMeasurementListResponse {
    pub data: Vec<StatefulTurnMeasurement>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS, ExperimentalApi)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMeasurementSummaryParams {
    pub project_id: String,
    /// Number of newest measurements to include in the bounded window.
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

/// Exact totals over a bounded newest-first window of project measurements.
///
/// Token counts are exposed without estimating monetary cost because the
/// persisted records do not contain authoritative provider pricing, units, or
/// currency.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMeasurementSummary {
    pub project_id: String,
    #[ts(type = "number")]
    pub measurement_count: u64,
    #[ts(type = "number")]
    pub run_count: u64,
    #[ts(type = "number")]
    pub terminal_measurement_count: u64,
    #[ts(type = "number")]
    pub completed_turns: u64,
    #[ts(type = "number")]
    pub failed_turns: u64,
    #[ts(type = "number")]
    pub aborted_turns: u64,
    #[ts(type = "number")]
    pub turns_with_token_usage: u64,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub counters: StatefulAttributionCounters,
    pub trajectory: Option<TurnTrajectory>,
    pub token_usage: Option<TokenUsageBreakdown>,
    #[ts(type = "number | null")]
    pub oldest_created_at: Option<i64>,
    #[ts(type = "number | null")]
    pub newest_created_at: Option<i64>,
    /// True when older project measurements exist outside this window.
    pub has_more: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulMeasurementSummaryResponse {
    pub summary: StatefulMeasurementSummary,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StatefulAttributionCompletedNotification {
    pub project_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub status: StatefulAttributionStatus,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub counters: StatefulAttributionCounters,
}
