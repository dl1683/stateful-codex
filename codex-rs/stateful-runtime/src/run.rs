use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

const MAX_ID_BYTES: usize = 512;
const MAX_PROJECT_ID_BYTES: usize = 512;
const MAX_THREAD_IDS: usize = 64;
const MAX_GOAL_BYTES: usize = 16 * 1024;
const MAX_STRATEGY_BYTES: usize = 16 * 1024;
const MAX_RESULT_BYTES: usize = 32 * 1024;
const MAX_PACKET_FIELD_BYTES: usize = 8 * 1024;
const MAX_PACKET_LIST_ITEMS: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct StatefulRunId(String);

impl StatefulRunId {
    pub fn parse(value: impl Into<String>) -> Result<Self, StatefulRunError> {
        let value = value.into();
        validate_identity(&value, MAX_ID_BYTES).map_err(|()| StatefulRunError::InvalidRunId)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StatefulRunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowMode {
    Autonomous,
    Collaborative,
    Socratic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatefulRunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Cancelled,
    Blocked,
    Failed,
}

impl StatefulRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewStatefulRun {
    pub project_id: String,
    pub thread_ids: Vec<String>,
    pub goal: String,
    pub mode: WorkflowMode,
}

impl NewStatefulRun {
    pub fn validate(&self) -> Result<(), StatefulRunError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| StatefulRunError::InvalidProjectId)?;
        if self.thread_ids.is_empty() || self.thread_ids.len() > MAX_THREAD_IDS {
            return Err(StatefulRunError::InvalidThreadIds);
        }
        for thread_id in &self.thread_ids {
            validate_identity(thread_id, MAX_ID_BYTES)
                .map_err(|()| StatefulRunError::InvalidThreadIds)?;
        }
        validate_text(&self.goal, MAX_GOAL_BYTES).map_err(|()| StatefulRunError::InvalidGoal)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulRun {
    pub id: StatefulRunId,
    pub value: NewStatefulRun,
    pub status: StatefulRunStatus,
    pub strategy: Option<String>,
    pub strategy_revision: u64,
    pub result: Option<String>,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulRunUpdate {
    pub expected_revision: u64,
    pub status: StatefulRunStatus,
    pub strategy: Option<String>,
    pub result: Option<String>,
}

impl StatefulRunUpdate {
    pub fn validate(&self) -> Result<(), StatefulRunError> {
        validate_optional_text(self.strategy.as_deref(), MAX_STRATEGY_BYTES)
            .map_err(|()| StatefulRunError::InvalidStrategy)?;
        validate_optional_text(self.result.as_deref(), MAX_RESULT_BYTES)
            .map_err(|()| StatefulRunError::InvalidResult)?;
        if self.status == StatefulRunStatus::Completed && self.result.is_none() {
            return Err(StatefulRunError::CompletedWithoutResult);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObligationPacket {
    pub examined: Vec<String>,
    pub rationale: Vec<String>,
    pub learning: Vec<String>,
    pub implication: Vec<String>,
    pub strategy: Vec<String>,
    pub changed: Vec<String>,
    pub next: Vec<String>,
    pub uncertainty: Vec<String>,
    pub blockers: Vec<String>,
    pub requested_judgment: Vec<String>,
}

impl ObligationPacket {
    pub fn validate(&self) -> Result<(), StatefulRunError> {
        let fields = [
            &self.examined,
            &self.rationale,
            &self.learning,
            &self.implication,
            &self.strategy,
            &self.changed,
            &self.next,
            &self.uncertainty,
            &self.blockers,
            &self.requested_judgment,
        ];
        if fields.iter().all(|items| items.is_empty()) {
            return Err(StatefulRunError::EmptyObligation);
        }
        for items in fields {
            if items.len() > MAX_PACKET_LIST_ITEMS
                || items
                    .iter()
                    .any(|item| validate_text(item, MAX_PACKET_FIELD_BYTES).is_err())
            {
                return Err(StatefulRunError::InvalidObligation);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewObligation {
    pub project_id: String,
    pub run_id: StatefulRunId,
    pub packet: ObligationPacket,
    pub provenance_source_id: String,
}

impl NewObligation {
    pub fn validate(&self) -> Result<(), StatefulRunError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| StatefulRunError::InvalidProjectId)?;
        validate_identity(&self.provenance_source_id, MAX_ID_BYTES)
            .map_err(|()| StatefulRunError::InvalidProvenance)?;
        self.packet.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulObligation {
    pub id: String,
    pub value: NewObligation,
    pub sequence: u64,
    pub revision: u64,
    pub created_at_ms: i64,
}

fn validate_identity(value: &str, maximum: usize) -> Result<(), ()> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(());
    }
    Ok(())
}

fn validate_text(value: &str, maximum: usize) -> Result<(), ()> {
    if value.is_empty() || value.len() > maximum || value.trim() != value || value.contains('\0') {
        return Err(());
    }
    Ok(())
}

fn validate_optional_text(value: Option<&str>, maximum: usize) -> Result<(), ()> {
    value.map_or(Ok(()), |value| validate_text(value, maximum))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum StatefulRunError {
    #[error("run ID must be non-empty, bounded, and contain no controls")]
    InvalidRunId,
    #[error("project ID must be non-empty, bounded, and contain no controls")]
    InvalidProjectId,
    #[error("run must contain 1-64 valid thread IDs")]
    InvalidThreadIds,
    #[error("run goal must be non-empty, bounded, trimmed, and contain no NUL")]
    InvalidGoal,
    #[error("run strategy must be bounded, trimmed, and contain no NUL")]
    InvalidStrategy,
    #[error("run result must be bounded, trimmed, and contain no NUL")]
    InvalidResult,
    #[error("completed run requires a result")]
    CompletedWithoutResult,
    #[error("obligation packet must contain at least one semantic field")]
    EmptyObligation,
    #[error("obligation packet fields must be bounded, trimmed, and contain no NUL")]
    InvalidObligation,
    #[error("obligation provenance must identify one bounded source")]
    InvalidProvenance,
}
