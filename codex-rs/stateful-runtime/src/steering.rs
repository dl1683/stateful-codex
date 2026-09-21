use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::StatefulRunId;

const MAX_ID_BYTES: usize = 512;
const MAX_PROJECT_ID_BYTES: usize = 512;
const MAX_INPUT_BYTES: usize = 16 * 1024;
const MAX_REASON_BYTES: usize = 8 * 1024;
const MAX_AFFECTED_OBLIGATIONS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SteeringId(String);

impl SteeringId {
    pub fn parse(value: impl Into<String>) -> Result<Self, SteeringError> {
        let value = value.into();
        validate_identity(&value, MAX_ID_BYTES).map_err(|()| SteeringError::InvalidId)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SteeringId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SteeringStatus {
    Submitted,
    Acknowledged,
    Applied,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewSteeringInstruction {
    pub project_id: String,
    pub run_id: StatefulRunId,
    pub input: String,
    pub affected_obligation_ids: Vec<String>,
}

impl NewSteeringInstruction {
    pub fn validate(&self) -> Result<(), SteeringError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| SteeringError::InvalidProjectId)?;
        validate_text(&self.input, MAX_INPUT_BYTES).map_err(|()| SteeringError::InvalidInput)?;
        if self.affected_obligation_ids.len() > MAX_AFFECTED_OBLIGATIONS {
            return Err(SteeringError::InvalidAffectedObligations);
        }
        let mut unique = HashSet::with_capacity(self.affected_obligation_ids.len());
        if self
            .affected_obligation_ids
            .iter()
            .any(|id| validate_identity(id, MAX_ID_BYTES).is_err() || !unique.insert(id.as_str()))
        {
            return Err(SteeringError::InvalidAffectedObligations);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatefulSteering {
    pub id: SteeringId,
    pub value: NewSteeringInstruction,
    pub status: SteeringStatus,
    pub resulting_strategy_revision: Option<u64>,
    pub reason: Option<String>,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteeringUpdate {
    pub expected_revision: u64,
    pub status: SteeringStatus,
    pub resulting_strategy_revision: Option<u64>,
    pub reason: Option<String>,
}

impl SteeringUpdate {
    pub fn validate(&self) -> Result<(), SteeringError> {
        if self
            .reason
            .as_ref()
            .is_some_and(|reason| validate_text(reason, MAX_REASON_BYTES).is_err())
        {
            return Err(SteeringError::InvalidReason);
        }
        match self.status {
            SteeringStatus::Applied if self.resulting_strategy_revision.is_none() => {
                Err(SteeringError::AppliedWithoutStrategyRevision)
            }
            SteeringStatus::Rejected if self.reason.is_none() => {
                Err(SteeringError::RejectedWithoutReason)
            }
            SteeringStatus::Submitted | SteeringStatus::Acknowledged
                if self.resulting_strategy_revision.is_some() =>
            {
                Err(SteeringError::PrematureStrategyRevision)
            }
            _ => Ok(()),
        }
    }
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

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SteeringError {
    #[error("steering ID must be non-empty, bounded, and contain no controls")]
    InvalidId,
    #[error("steering project ID must be non-empty, bounded, and contain no controls")]
    InvalidProjectId,
    #[error("steering input must be non-empty, bounded, trimmed, and contain no NUL")]
    InvalidInput,
    #[error("affected obligation IDs must be unique, valid, and bounded")]
    InvalidAffectedObligations,
    #[error("steering reason must be non-empty, bounded, trimmed, and contain no NUL")]
    InvalidReason,
    #[error("applied steering requires the resulting strategy revision")]
    AppliedWithoutStrategyRevision,
    #[error("rejected steering requires a reason")]
    RejectedWithoutReason,
    #[error("steering cannot name a resulting strategy revision before application")]
    PrematureStrategyRevision,
}
