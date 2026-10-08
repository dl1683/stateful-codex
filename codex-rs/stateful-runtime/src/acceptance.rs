//! The run-owned acceptance ledger and the completion gate it enforces.
//!
//! A ledger lists what the user's request requires (deliverables, constraints, measurable
//! checks and items no host can check), each user criterion linked to the exact byte span of
//! the run goal it came from. Evidence is host-observed (a check command's exit status and
//! bounded output), a labelled manual observation, or an explicit statement that no safe
//! check exists. A model assertion is never evidence: the gate below decides whether every
//! criterion is satisfied with current evidence or explicitly disclosed as unverified.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::StatefulRunId;

/// Criteria a ledger may hold in any state, so rendering and gating stay bounded.
pub const MAX_ACCEPTANCE_CRITERIA: usize = 32;
/// Changes one revision may apply.
pub const MAX_ACCEPTANCE_CHANGES: usize = 8;
/// Artifact paths one criterion may declare.
pub const MAX_CRITERION_ARTIFACTS: usize = 8;
pub const MAX_STATEMENT_BYTES: usize = 1024;
pub const MAX_ARTIFACT_PATH_BYTES: usize = 512;
pub const MAX_CHECK_COMMAND_BYTES: usize = 1024;
pub const MAX_REASON_BYTES: usize = 1024;
/// Stored tail of a check's combined output; the full output is kept only as a digest.
pub const MAX_OUTPUT_TAIL_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceOrigin {
    /// Stated by the user; linked to a goal span and never weakened or retired.
    User,
    /// Derived by the agent; refinable and retirable with a reason.
    Derived,
    /// Proposed by the host omission check from an uncovered goal span.
    Omission,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceKind {
    Deliverable,
    Constraint,
    Check,
    Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceState {
    Active,
    /// An omission proposal awaiting accept or dismiss; blocks completion.
    Proposed,
    Dismissed,
    Retired,
}

/// Byte range `[start, end)` of the run goal a criterion came from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSpan {
    pub start: usize,
    pub end: usize,
}

impl RequestSpan {
    /// The exact goal text of this span, when it is a non-empty UTF-8 range of `goal`.
    pub fn quote<'a>(&self, goal: &'a str) -> Option<&'a str> {
        (self.start < self.end)
            .then(|| goal.get(self.start..self.end))
            .flatten()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceSource {
    /// The host observed a check command run through the session's own shell tool.
    HostCommand,
    /// A labelled observation written by the agent; never counts as host verification.
    Manual,
    /// No safe check exists or the check could not run; carries the reason.
    NoCheck,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceOutcome {
    Passed,
    Failed,
    Observed,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceEvidence {
    pub sequence: u64,
    pub source: EvidenceSource,
    pub outcome: EvidenceOutcome,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub output_tail: Option<String>,
    pub output_digest: Option<String>,
    pub artifact_digest: Option<String>,
    pub detail: Option<String>,
    pub workspace_generation: u64,
    pub criterion_revision: u64,
    pub source_id: String,
    pub observed_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCriterion {
    pub ordinal: u32,
    pub origin: AcceptanceOrigin,
    pub kind: AcceptanceKind,
    pub state: AcceptanceState,
    pub statement: String,
    pub request_span: Option<RequestSpan>,
    pub artifacts: Vec<String>,
    pub check_command: Option<String>,
    pub note: Option<String>,
    pub revision: u64,
    /// The newest evidence recorded for this criterion, of any revision.
    pub evidence: Option<AcceptanceEvidence>,
}

impl AcceptanceCriterion {
    /// `C<ordinal>`, the alias the model and rendering use.
    pub fn alias(&self) -> String {
        format!("C{}", self.ordinal)
    }

    /// Statement, span, retirement and existing artifacts are fixed for criteria that come
    /// from the user's words.
    pub fn is_user_bound(&self) -> bool {
        matches!(
            self.origin,
            AcceptanceOrigin::User | AcceptanceOrigin::Omission
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceLedger {
    pub run_id: StatefulRunId,
    /// Incremented by every criterion change and every recorded evidence.
    pub revision: u64,
    /// Incremented by every host-observed workspace mutation that is not a declared check.
    pub workspace_generation: u64,
    pub omission_checked: bool,
    /// Consecutive rejected completions with no ledger change in between.
    pub stalled_completions: u32,
    pub criteria: Vec<AcceptanceCriterion>,
}

impl AcceptanceLedger {
    pub fn empty(run_id: StatefulRunId) -> Self {
        Self {
            run_id,
            revision: 0,
            workspace_generation: 0,
            omission_checked: false,
            stalled_completions: 0,
            criteria: Vec::new(),
        }
    }

    pub fn criterion(&self, ordinal: u32) -> Option<&AcceptanceCriterion> {
        self.criteria
            .iter()
            .find(|criterion| criterion.ordinal == ordinal)
    }
}

/// One requested change to a ledger. Omission proposals are host-only and are added through
/// `StatefulRunStore::record_omission_check`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptanceChange {
    Add {
        origin: AcceptanceOrigin,
        kind: AcceptanceKind,
        statement: String,
        request_span: Option<RequestSpan>,
        artifacts: Vec<String>,
        check_command: Option<String>,
    },
    Refine {
        ordinal: u32,
        statement: Option<String>,
        artifacts: Option<Vec<String>>,
        check_command: Option<String>,
    },
    Accept {
        ordinal: u32,
        kind: Option<AcceptanceKind>,
        artifacts: Option<Vec<String>>,
        check_command: Option<String>,
    },
    Dismiss {
        ordinal: u32,
        reason: String,
    },
    Retire {
        ordinal: u32,
        reason: String,
    },
    Observe {
        ordinal: u32,
        observation: String,
    },
    NoCheck {
        ordinal: u32,
        reason: String,
    },
}

/// A check command execution the host observed, bound to one criterion revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandEvidence {
    pub ordinal: u32,
    pub criterion_revision: u64,
    pub outcome: EvidenceOutcome,
    pub command: String,
    pub exit_code: Option<i32>,
    pub output_tail: String,
    pub output_digest: String,
    pub artifact_digest: Option<String>,
    pub detail: Option<String>,
    pub source_id: String,
}

/// The host's reading of a criterion's declared artifacts at completion time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactState {
    Observed {
        digest: String,
        missing: Vec<String>,
    },
    Unavailable(String),
}

/// What a completion commit was validated against; the store re-checks it inside the
/// terminal transaction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AcceptanceCommit {
    pub ledger_revision: u64,
    pub workspace_generation: u64,
    pub artifacts: BTreeMap<u32, ArtifactState>,
    /// Whether policy required the omission check before this completion.
    pub omission_required: bool,
}

/// How one criterion stands against the gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CriterionVerdict {
    /// A host-observed check passed against the current workspace.
    SatisfiedByHost,
    /// Every declared artifact is present now; content was not checked.
    ArtifactsPresent,
    /// A labelled manual observation, current with the workspace.
    ManualObservation,
    /// Explicitly unverified, with the reason disclosed in the completion.
    DisclosedUnverified(String),
    /// Not gating: dismissed or retired with a disclosed reason.
    Closed(String),
    /// Blocks completion; the text says exactly what is missing or failed.
    Unmet(String),
}

impl CriterionVerdict {
    pub fn is_unmet(&self) -> bool {
        matches!(self, Self::Unmet(_))
    }
}

/// Judges one criterion against the workspace generation and, for criteria that declare
/// artifacts, the host's current reading of them.
pub fn criterion_verdict(
    criterion: &AcceptanceCriterion,
    workspace_generation: u64,
    artifacts: Option<&ArtifactState>,
) -> CriterionVerdict {
    match criterion.state {
        AcceptanceState::Proposed => {
            return CriterionVerdict::Unmet(
                "omission proposal awaiting review: accept it as a criterion or dismiss it with a reason".to_string(),
            );
        }
        AcceptanceState::Dismissed | AcceptanceState::Retired => {
            return CriterionVerdict::Closed(criterion.note.clone().unwrap_or_default());
        }
        AcceptanceState::Active => {}
    }
    let current = criterion
        .evidence
        .as_ref()
        .filter(|evidence| evidence.criterion_revision == criterion.revision);
    let artifact_reading = || -> Result<Option<&str>, String> {
        if criterion.artifacts.is_empty() {
            return Ok(None);
        }
        match artifacts {
            Some(ArtifactState::Observed { digest, missing }) if missing.is_empty() => {
                Ok(Some(digest.as_str()))
            }
            Some(ArtifactState::Observed { missing, .. }) => Err(format!(
                "declared artifact {} is missing",
                missing.join(", ")
            )),
            Some(ArtifactState::Unavailable(reason)) => {
                Err(format!("declared artifacts could not be read: {reason}"))
            }
            None => Err("declared artifacts were not read for this completion".to_string()),
        }
    };
    if let Some(command) = criterion.check_command.as_deref() {
        let Some(evidence) = current else {
            return CriterionVerdict::Unmet(format!(
                "no current host evidence: run exactly `{command}` with the shell tool; the host records its exit status"
            ));
        };
        return match evidence.outcome {
            EvidenceOutcome::Failed => CriterionVerdict::Unmet(format!(
                "`{command}` failed (exit {}); repair the work and run it again",
                evidence
                    .exit_code
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string())
            )),
            EvidenceOutcome::Unavailable => CriterionVerdict::DisclosedUnverified(
                evidence
                    .detail
                    .clone()
                    .unwrap_or_else(|| "the check could not run".to_string()),
            ),
            EvidenceOutcome::Passed | EvidenceOutcome::Observed => {
                if evidence.workspace_generation != workspace_generation {
                    return CriterionVerdict::Unmet(format!(
                        "stale: the workspace changed after `{command}` passed; run it again"
                    ));
                }
                match artifact_reading() {
                    Err(reason) => CriterionVerdict::Unmet(reason),
                    Ok(Some(digest)) if evidence.artifact_digest.as_deref() != Some(digest) => {
                        CriterionVerdict::Unmet(format!(
                            "stale: declared artifacts changed after `{command}` passed; run it again"
                        ))
                    }
                    Ok(_) => CriterionVerdict::SatisfiedByHost,
                }
            }
        };
    }
    match current.map(|evidence| (evidence.outcome, evidence)) {
        Some((EvidenceOutcome::Unavailable, evidence)) => CriterionVerdict::DisclosedUnverified(
            evidence
                .detail
                .clone()
                .unwrap_or_else(|| "no safe check is available".to_string()),
        ),
        Some((EvidenceOutcome::Observed, evidence))
            if evidence.workspace_generation == workspace_generation =>
        {
            match artifact_reading() {
                Err(reason) => CriterionVerdict::Unmet(reason),
                Ok(_) => CriterionVerdict::ManualObservation,
            }
        }
        Some((EvidenceOutcome::Observed, _)) => CriterionVerdict::Unmet(
            "stale: the workspace changed after the manual observation; observe it again"
                .to_string(),
        ),
        _ if criterion.kind == AcceptanceKind::Deliverable && !criterion.artifacts.is_empty() => {
            match artifact_reading() {
                Err(reason) => CriterionVerdict::Unmet(reason),
                Ok(_) => CriterionVerdict::ArtifactsPresent,
            }
        }
        _ => CriterionVerdict::Unmet(
            "no evidence: add a checkCommand and run it, record a labelled manual observation, or record noCheck with the reason no safe check exists".to_string(),
        ),
    }
}

/// Every criterion that blocks completion, with the reason, in ordinal order.
pub fn unmet_criteria(ledger: &AcceptanceLedger, commit: &AcceptanceCommit) -> Vec<(u32, String)> {
    ledger
        .criteria
        .iter()
        .filter_map(|criterion| {
            match criterion_verdict(
                criterion,
                ledger.workspace_generation,
                commit.artifacts.get(&criterion.ordinal),
            ) {
                CriterionVerdict::Unmet(reason) => Some((criterion.ordinal, reason)),
                CriterionVerdict::SatisfiedByHost
                | CriterionVerdict::ArtifactsPresent
                | CriterionVerdict::ManualObservation
                | CriterionVerdict::DisclosedUnverified(_)
                | CriterionVerdict::Closed(_) => None,
            }
        })
        .collect()
}

pub(crate) fn validate_bounded(value: &str, maximum: usize) -> Result<(), AcceptanceError> {
    if value.is_empty()
        || value.len() > maximum
        || value.trim() != value
        || value.chars().any(|character| character == '\0')
    {
        return Err(AcceptanceError::InvalidText(maximum));
    }
    Ok(())
}

/// A check command must be one bounded line: the host matches it verbatim against the
/// command the shell tool actually ran.
pub(crate) fn validate_check_command(value: &str) -> Result<(), AcceptanceError> {
    validate_bounded(value, MAX_CHECK_COMMAND_BYTES)?;
    if value.chars().any(char::is_control) {
        return Err(AcceptanceError::UnsupportedCheckCommand);
    }
    Ok(())
}

/// Artifact paths are relative to a project root, or absolute inside one; the extension
/// resolves them. Here they are only bounded, single-line and free of parent traversal.
pub(crate) fn validate_artifacts(artifacts: &[String]) -> Result<(), AcceptanceError> {
    if artifacts.len() > MAX_CRITERION_ARTIFACTS {
        return Err(AcceptanceError::TooManyArtifacts);
    }
    for artifact in artifacts {
        validate_bounded(artifact, MAX_ARTIFACT_PATH_BYTES)?;
        if artifact.chars().any(char::is_control)
            || artifact
                .split(['/', '\\'])
                .any(|component| component == "..")
        {
            return Err(AcceptanceError::InvalidArtifact(artifact.clone()));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AcceptanceError {
    #[error("acceptance text must be non-empty, trimmed, at most {0} bytes, and contain no NUL")]
    InvalidText(usize),
    #[error(
        "checkCommand must be one line the host can match verbatim against the shell tool's command; multi-line or control-character commands are unsupported"
    )]
    UnsupportedCheckCommand,
    #[error("a criterion may declare at most {MAX_CRITERION_ARTIFACTS} artifacts")]
    TooManyArtifacts,
    #[error("artifact path {0:?} must be one line without parent-directory components")]
    InvalidArtifact(String),
    #[error("a ledger holds at most {MAX_ACCEPTANCE_CRITERIA} criteria")]
    TooManyCriteria,
    #[error("one acceptance update applies 1-{MAX_ACCEPTANCE_CHANGES} changes")]
    InvalidChangeCount,
    #[error("unknown criterion C{0}")]
    UnknownCriterion(u32),
    #[error("{0}")]
    Refused(String),
    #[error("request span is not an exact non-empty range of the run goal")]
    InvalidSpan,
}
