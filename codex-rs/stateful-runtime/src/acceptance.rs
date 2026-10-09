//! The run-owned acceptance ledger and the completion gate it enforces.
//!
//! A ledger lists what the user's request requires. User criteria are linked to the exact
//! byte span of the run goal they quote; every goal sentence must be covered by such a
//! criterion (or by a host omission proposal) before a substantial run can complete. A
//! proposal can be dismissed only as covered by an active required user criterion; there is
//! no model-interpreted waiver.
//!
//! The gate is conservative: when evidence cannot be qualified it stays unmet, and the run
//! ends `Blocked` with a partial result instead of `Completed`.
//!
//! - A host receipt (an observed check execution) proves what ran, in which directory, and
//!   its exit status. It settles a criterion only against the criterion's host-admitted check
//!   plan: the host freezes, from the ledger, the exact criterion revision, check command and
//!   directory, the checker files the command names (content-frozen), and the pinned
//!   artifacts. The receipt must have run that plan with the frozen checker bytes, against
//!   the current pinned artifacts and workspace generation. Any change of scope, method,
//!   checker or dependencies invalidates the admission. No user approval is involved, so
//!   Autonomous runs can complete unattended; the admission can never be minted from quoted
//!   text.
//! - Existence evidence settles only derived `existence` criteria; manual observations settle
//!   only derived `manual` criteria and only with pinned artifact content.
//! - Unverified required work is unmet; only optional criteria may complete
//!   disclosed-unverified.

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
/// Distinct earlier criteria one criterion may depend on.
pub const MAX_CRITERION_DEPENDENCIES: usize = 8;
pub const MAX_STATEMENT_BYTES: usize = 1024;
pub const MAX_ARTIFACT_PATH_BYTES: usize = 512;
pub const MAX_CHECK_COMMAND_BYTES: usize = 1024;
pub const MAX_REASON_BYTES: usize = 1024;
pub const MAX_MILESTONE_BYTES: usize = 160;
/// Stored tail of a check's combined output; the full output is kept only as a digest.
pub const MAX_OUTPUT_TAIL_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceOrigin {
    /// Stated by the user; linked to a goal span and never weakened or retired.
    User,
    /// Derived by the agent; refinable and retirable, and never covers a user requirement.
    Derived,
    /// Proposed by the host omission check from an uncovered goal sentence.
    Omission,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceKind {
    Deliverable,
    Constraint,
    Check,
    Manual,
    /// Only that the declared artifacts exist; says nothing about their content.
    Existence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceState {
    Active,
    /// An omission proposal awaiting accept or a receipted dismissal; blocks completion.
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

/// Why a user-bound omission proposal no longer gates: an active required user-bound
/// criterion whose span already covers it. Re-validated at every evaluation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DismissalReceipt {
    CoveredBy(u32),
}

/// The host's admission of a criterion's check plan, frozen from the ledger.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAdmission {
    /// The criterion revision (command, directory, checker, artifacts, expectation) admitted.
    pub criterion_revision: u64,
    /// Content digest of the checker files at admission.
    pub checker_digest: String,
    pub ledger_revision: u64,
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
    /// Content digest of the criterion's artifacts when the check started (or when the
    /// manual observation was recorded).
    pub artifact_digest: Option<String>,
    /// Content digest of the criterion's checker files when the check started.
    pub checker_digest: Option<String>,
    pub detail: Option<String>,
    /// The workspace generation when the check started.
    pub workspace_generation: u64,
    pub criterion_revision: u64,
    pub source_id: String,
    pub observed_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCriterion {
    /// Stable for the life of the run: `<run id>#C<ordinal>`.
    pub id: String,
    pub ordinal: u32,
    pub origin: AcceptanceOrigin,
    pub kind: AcceptanceKind,
    pub state: AcceptanceState,
    /// The agent's operational statement of the criterion.
    pub statement: String,
    /// The requirement itself: the exact goal text for user and omission criteria, the
    /// statement for derived ones.
    pub requirement: String,
    /// Required criteria gate completion; optional ones may complete disclosed-unverified.
    pub required: bool,
    /// Distinct earlier criteria (by ordinal) that must be met before this one counts as met.
    pub depends_on: Vec<u32>,
    /// The irreversible step (approval, filing, publication, ...) this criterion must be
    /// verified before. A label only; the action itself is not gated.
    pub milestone: Option<String>,
    pub request_span: Option<RequestSpan>,
    /// The files this criterion's evidence is pinned to (inputs and outputs).
    pub artifacts: Vec<String>,
    /// The checker files the check command runs (tests, scripts); content-frozen at admission.
    pub checker: Vec<String>,
    pub check_command: Option<String>,
    /// The working directory the check must run in, relative to the first project root or
    /// absolute inside a project root; the first root when absent.
    pub check_cwd: Option<String>,
    /// What a passing check must show for it to establish the requirement.
    pub expected_observation: Option<String>,
    /// The host-admitted check plan, if any.
    pub plan: Option<PlanAdmission>,
    pub dismissal: Option<DismissalReceipt>,
    pub note: Option<String>,
    pub revision: u64,
    /// The ledger revision that created or last changed this criterion.
    pub ledger_revision: u64,
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

/// An applied steering instruction the ledger reconciled. The reason is agent-written and is
/// disclosed in the completion basis.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteeringReconciliation {
    pub steering_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceLedger {
    pub run_id: StatefulRunId,
    /// Incremented only by actual criterion changes and recorded evidence.
    pub revision: u64,
    /// Incremented by every host-observed workspace mutation and by every cold re-entry of a
    /// run that holds evidence.
    pub workspace_generation: u64,
    /// Command executions the host observed for this run, read-only ones included.
    pub observed_executions: u64,
    /// Consecutive rejected completions with no ledger change in between.
    pub stalled_completions: u32,
    /// Commands started for this run whose effects are not accounted for yet.
    pub pending_commands: u64,
    /// Applied steering instructions this ledger has reconciled, with the agent's reason.
    pub reconciled_steering: Vec<SteeringReconciliation>,
    /// Set when the host admitted the run as a cheap lookup that owes no coverage.
    pub cheap_lookup: bool,
    /// Completion verification attempts started for this run.
    pub verification_attempt: u64,
    /// Lease of the attempt in progress; the run stays `Running` meanwhile.
    pub verification_lease_expires_at_ms: Option<i64>,
    pub criteria: Vec<AcceptanceCriterion>,
}

impl AcceptanceLedger {
    pub fn empty(run_id: StatefulRunId) -> Self {
        Self {
            run_id,
            revision: 0,
            workspace_generation: 0,
            observed_executions: 0,
            stalled_completions: 0,
            pending_commands: 0,
            reconciled_steering: Vec::new(),
            cheap_lookup: false,
            verification_attempt: 0,
            verification_lease_expires_at_ms: None,
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
/// `StatefulRunStore::propose_uncovered`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptanceChange {
    Add {
        origin: AcceptanceOrigin,
        kind: AcceptanceKind,
        statement: String,
        request_span: Option<RequestSpan>,
        terms: CriterionTerms,
    },
    Refine {
        ordinal: u32,
        statement: Option<String>,
        required: Option<bool>,
        terms: TermsUpdate,
    },
    Accept {
        ordinal: u32,
        kind: Option<AcceptanceKind>,
        terms: TermsUpdate,
    },
    Dismiss {
        ordinal: u32,
        reason: String,
        receipt: DismissalReceipt,
    },
    Retire {
        ordinal: u32,
        reason: String,
    },
    /// Asks the host to admit the criterion's current check plan; `checker_digest` is the
    /// host's own reading of the checker files (absent when they could not be read).
    Admit {
        ordinal: u32,
        checker_digest: Option<String>,
    },
    /// Records how an applied steering instruction affects acceptance; invalidates every
    /// admitted plan and earlier evidence, since the scope may have changed.
    ReconcileSteering {
        steering_id: String,
        reason: String,
    },
    Observe {
        ordinal: u32,
        observation: String,
        /// Host-read content digest of the criterion's artifacts at observation time.
        artifact_digest: Option<String>,
    },
    NoCheck {
        ordinal: u32,
        reason: String,
    },
}

/// How a new criterion is checked and ordered.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CriterionTerms {
    pub required: bool,
    pub depends_on: Vec<u32>,
    pub milestone: Option<String>,
    pub artifacts: Vec<String>,
    pub checker: Vec<String>,
    pub check_command: Option<String>,
    pub check_cwd: Option<String>,
    pub expected_observation: Option<String>,
}

/// Additions or replacements to a criterion's terms; `None` keeps the current value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TermsUpdate {
    pub depends_on: Option<Vec<u32>>,
    pub milestone: Option<String>,
    pub artifacts: Option<Vec<String>>,
    pub checker: Option<Vec<String>>,
    pub check_command: Option<String>,
    pub check_cwd: Option<String>,
    pub expected_observation: Option<String>,
}

/// A check command execution the host observed, bound to one criterion revision and to the
/// workspace generation and artifact content when it started.
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
    pub checker_digest: Option<String>,
    pub detail: Option<String>,
    pub start_generation: u64,
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

/// The verification attempt a completion commit consumes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationClaim {
    pub owner: String,
    pub attempt: u64,
}

/// What a completion commit was validated against; the store re-derives the policy and
/// re-checks all of it inside the terminal transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceCommit {
    pub ledger_revision: u64,
    pub workspace_generation: u64,
    pub artifacts: BTreeMap<u32, ArtifactState>,
    /// The host's reading of each criterion's checker files.
    pub checkers: BTreeMap<u32, ArtifactState>,
    pub verification: VerificationClaim,
    /// The latest recorded obligation sequence the caller checked for open blockers.
    pub validated_obligation_sequence: Option<u64>,
}

/// How one criterion stands against the gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CriterionVerdict {
    /// A host receipt of the admitted check plan passed against the current pinned artifacts
    /// and the frozen checker.
    SatisfiedByHost,
    /// A derived existence criterion: every declared artifact is present now.
    ArtifactsPresent,
    /// A derived manual criterion with an observation pinned to the current artifacts.
    ManualObservation,
    /// An optional criterion explicitly unverified, with the reason disclosed in the
    /// completion. Required criteria are never in this state.
    DisclosedUnverified(String),
    /// Not gating: a receipted dismissal or a retired derived criterion.
    Closed(String),
    /// Blocks completion; the text says exactly what is missing or failed.
    Unmet(String),
}

impl CriterionVerdict {
    pub fn is_unmet(&self) -> bool {
        matches!(self, Self::Unmet(_))
    }
}

/// Judges one criterion against the workspace generation and the host's current reading of
/// its declared artifacts.
pub fn criterion_verdict(
    criterion: &AcceptanceCriterion,
    workspace_generation: u64,
    artifacts: Option<&ArtifactState>,
    checker: Option<&ArtifactState>,
) -> CriterionVerdict {
    match criterion.state {
        AcceptanceState::Proposed => {
            return CriterionVerdict::Unmet(
                "omission proposal awaiting review: accept it as a criterion, or dismiss it only as coveredBy a user criterion".to_string(),
            );
        }
        // A dismissal's receipt is re-validated by the ledger-level verdicts.
        AcceptanceState::Dismissed | AcceptanceState::Retired => {
            return CriterionVerdict::Closed(criterion.note.clone().unwrap_or_default());
        }
        AcceptanceState::Active => {}
    }
    let current = criterion
        .evidence
        .as_ref()
        .filter(|evidence| evidence.criterion_revision == criterion.revision);
    // The current content digest of the pinned artifacts; every grade below needs one.
    let pinned = || -> Result<&str, String> {
        if criterion.artifacts.is_empty() {
            return Err(
                "no artifacts are pinned, so no evidence can be current; declare the files this criterion covers"
                    .to_string(),
            );
        }
        match artifacts {
            Some(ArtifactState::Observed { digest, missing }) if missing.is_empty() => {
                Ok(digest.as_str())
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
                "no current host evidence: run exactly `{command}` with the shell tool in its check directory"
            ));
        };
        return match evidence.outcome {
            EvidenceOutcome::Failed => CriterionVerdict::Unmet(format!(
                "`{command}` failed (exit {}); repair the work and run it again",
                evidence
                    .exit_code
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string())
            )),
            EvidenceOutcome::Unavailable | EvidenceOutcome::Observed => unverified(
                criterion,
                evidence
                    .detail
                    .clone()
                    .unwrap_or_else(|| "the check could not run".to_string()),
            ),
            EvidenceOutcome::Passed => {
                if evidence.workspace_generation != workspace_generation {
                    return CriterionVerdict::Unmet(format!(
                        "stale: the workspace changed after `{command}` started; run it again"
                    ));
                }
                match pinned() {
                    Err(reason) => CriterionVerdict::Unmet(reason),
                    Ok(digest) if evidence.artifact_digest.as_deref() != Some(digest) => {
                        CriterionVerdict::Unmet(format!(
                            "stale: the pinned artifacts changed after `{command}` started; run it again"
                        ))
                    }
                    Ok(_) => match plan_status(criterion, evidence, checker) {
                        Ok(()) => CriterionVerdict::SatisfiedByHost,
                        Err(reason) => CriterionVerdict::Unmet(reason),
                    },
                }
            }
        };
    }
    if let Some(evidence) = current
        && evidence.outcome == EvidenceOutcome::Unavailable
    {
        return unverified(
            criterion,
            evidence
                .detail
                .clone()
                .unwrap_or_else(|| "no safe check is available".to_string()),
        );
    }
    let derived = criterion.origin == AcceptanceOrigin::Derived;
    match criterion.kind {
        AcceptanceKind::Existence if derived => match pinned() {
            Ok(_) => CriterionVerdict::ArtifactsPresent,
            Err(reason) => CriterionVerdict::Unmet(reason),
        },
        AcceptanceKind::Manual if derived => match current {
            Some(evidence) if evidence.outcome == EvidenceOutcome::Observed => {
                if evidence.workspace_generation != workspace_generation {
                    return CriterionVerdict::Unmet(
                        "stale: the workspace changed after the manual observation; observe it again"
                            .to_string(),
                    );
                }
                match pinned() {
                    Err(reason) => CriterionVerdict::Unmet(reason),
                    Ok(digest) if evidence.artifact_digest.as_deref() != Some(digest) => {
                        CriterionVerdict::Unmet(
                            "stale: the observed artifacts changed after the manual observation; observe them again"
                                .to_string(),
                        )
                    }
                    Ok(_) => CriterionVerdict::ManualObservation,
                }
            }
            _ => CriterionVerdict::Unmet(
                "no current manual observation pinned to the declared artifacts".to_string(),
            ),
        },
        AcceptanceKind::Existence | AcceptanceKind::Manual if !derived => unverified(
            criterion,
            "a user requirement is settled only by a receipt of its host-admitted check plan".to_string(),
        ),
        AcceptanceKind::Deliverable
        | AcceptanceKind::Constraint
        | AcceptanceKind::Check
        | AcceptanceKind::Existence
        | AcceptanceKind::Manual => unverified(
            criterion,
            "no check: add a checkCommand naming its checker files, pin the artifacts, and admit the plan"
                .to_string(),
        ),
    }
}

/// Whether a passing receipt ran the criterion's current host-admitted plan with the frozen
/// checker, and the checker is still unchanged now.
fn plan_status(
    criterion: &AcceptanceCriterion,
    evidence: &AcceptanceEvidence,
    checker: Option<&ArtifactState>,
) -> Result<(), String> {
    let command = criterion.check_command.as_deref().unwrap_or_default();
    let Some(plan) = &criterion.plan else {
        return Err(format!(
            "`{command}` passed, but a passing exit only shows what ran; it settles the criterion only after the host admits its check plan (stateful_acceptance_update admit). Without an admitted plan, end the run blocked with a partial result"
        ));
    };
    if plan.criterion_revision != criterion.revision {
        return Err("the check plan changed after its admission; admit it again".to_string());
    }
    let current = match checker {
        Some(ArtifactState::Observed { digest, missing }) if missing.is_empty() => digest,
        Some(ArtifactState::Observed { missing, .. }) => {
            return Err(format!("checker file {} is missing", missing.join(", ")));
        }
        Some(ArtifactState::Unavailable(reason)) => {
            return Err(format!("the checker files could not be read: {reason}"));
        }
        None => return Err("the checker files were not read for this completion".to_string()),
    };
    if *current != plan.checker_digest {
        return Err("the checker changed after its plan was admitted; admit it again".to_string());
    }
    if evidence.checker_digest.as_deref() != Some(plan.checker_digest.as_str()) {
        return Err(format!(
            "`{command}` did not run the admitted checker bytes; run it again"
        ));
    }
    Ok(())
}

/// Unverified required work is unmet: disclosure is a partial outcome, never completion.
fn unverified(criterion: &AcceptanceCriterion, reason: String) -> CriterionVerdict {
    if criterion.required {
        CriterionVerdict::Unmet(format!(
            "required but unverified ({reason}); required work cannot complete unverified. Repair it or obtain an approved check; otherwise set the run blocked with a partial result that names it"
        ))
    } else {
        CriterionVerdict::DisclosedUnverified(reason)
    }
}

/// Every criterion's verdict in ordinal order. A criterion whose dependency is unmet is unmet
/// itself, whatever its own evidence says.
pub fn ledger_verdicts(
    ledger: &AcceptanceLedger,
    artifacts: &BTreeMap<u32, ArtifactState>,
    checkers: &BTreeMap<u32, ArtifactState>,
) -> Vec<(u32, CriterionVerdict)> {
    let mut verdicts: Vec<(u32, CriterionVerdict)> = Vec::with_capacity(ledger.criteria.len());
    for criterion in &ledger.criteria {
        let verdict = match (criterion.state, &criterion.dismissal) {
            (AcceptanceState::Dismissed, Some(DismissalReceipt::CoveredBy(covering)))
                if !crate::acceptance_coverage::covers(ledger, *covering, criterion) =>
            {
                CriterionVerdict::Unmet(format!(
                    "its dismissal relied on C{covering}, which no longer covers it"
                ))
            }
            _ => criterion_verdict(
                criterion,
                ledger.workspace_generation,
                artifacts.get(&criterion.ordinal),
                checkers.get(&criterion.ordinal),
            ),
        };
        let blocked_by = (criterion.state == AcceptanceState::Active)
            .then(|| {
                criterion.depends_on.iter().copied().find(|dependency| {
                    verdicts
                        .iter()
                        .any(|(ordinal, verdict)| ordinal == dependency && verdict.is_unmet())
                })
            })
            .flatten();
        verdicts.push((
            criterion.ordinal,
            match blocked_by {
                Some(dependency) if !verdict.is_unmet() => {
                    CriterionVerdict::Unmet(format!("depends on C{dependency}, which is unmet"))
                }
                _ => verdict,
            },
        ));
    }
    verdicts
}

/// Every criterion that blocks completion, with the reason, in ordinal order.
pub fn unmet_criteria(
    ledger: &AcceptanceLedger,
    artifacts: &BTreeMap<u32, ArtifactState>,
    checkers: &BTreeMap<u32, ArtifactState>,
) -> Vec<(u32, String)> {
    ledger_verdicts(ledger, artifacts, checkers)
        .into_iter()
        .filter_map(|(ordinal, verdict)| match verdict {
            CriterionVerdict::Unmet(reason) => Some((ordinal, reason)),
            CriterionVerdict::SatisfiedByHost
            | CriterionVerdict::ArtifactsPresent
            | CriterionVerdict::ManualObservation
            | CriterionVerdict::DisclosedUnverified(_)
            | CriterionVerdict::Closed(_) => None,
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

/// Paths are relative to a project root, or absolute inside one; the extension resolves
/// them. Here they are only bounded, single-line and free of parent traversal.
pub(crate) fn validate_path(path: &str) -> Result<(), AcceptanceError> {
    validate_bounded(path, MAX_ARTIFACT_PATH_BYTES)?;
    if path.chars().any(char::is_control)
        || path.split(['/', '\\']).any(|component| component == "..")
    {
        return Err(AcceptanceError::InvalidArtifact(path.to_string()));
    }
    Ok(())
}

pub(crate) fn validate_artifacts(artifacts: &[String]) -> Result<(), AcceptanceError> {
    if artifacts.len() > MAX_CRITERION_ARTIFACTS {
        return Err(AcceptanceError::TooManyArtifacts);
    }
    for artifact in artifacts {
        validate_path(artifact)?;
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
    #[error("path {0:?} must be one line without parent-directory components")]
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
    #[error(
        "a checkCommand needs expectedObservation (what its passing output must show) and at least one pinned artifact"
    )]
    CheckWithoutExpectation,
    #[error(
        "checker may name at most {MAX_CRITERION_ARTIFACTS} files, none of them a pinned artifact"
    )]
    InvalidChecker,
    #[error(
        "dependsOn may name at most {MAX_CRITERION_DEPENDENCIES} distinct earlier criteria of this ledger (no duplicates, self or forward references)"
    )]
    InvalidDependency,
    #[error("another completion verification holds the lease until {0}; retry after it ends")]
    VerificationLeased(i64),
}
