use crate::JsonSchema;
use crate::TS;
use codex_experimental_api_macros::ExperimentalApi;
use serde::Deserialize;
use serde::Serialize;

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
run_control_response!(StatefulRunCancelResponse);

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
