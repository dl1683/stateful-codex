use std::collections::HashSet;
use std::fmt;

use thiserror::Error;

const MAX_ID_BYTES: usize = 512;
const MAX_PROJECT_ID_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 4096;
const MAX_REF_BYTES: usize = 1024;
const MAX_DIGEST_BYTES: usize = 512;
/// Maximum roots one project observation may collect; excess roots are omitted.
const MAX_OBSERVED_ROOTS: usize = 32;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryObservationId(String);

impl RepositoryObservationId {
    pub fn parse(value: impl Into<String>) -> Result<Self, RepositoryObservationError> {
        let value = value.into();
        if !is_bounded_text(&value, MAX_ID_BYTES) {
            return Err(RepositoryObservationError::InvalidId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepositoryObservationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// One immutable project snapshot, containing one observation per collected root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryObservation {
    pub id: RepositoryObservationId,
    pub project_id: String,
    pub started_at_ms: i64,
    pub completed_at_ms: i64,
    /// Length-framed digest of the sorted, deduplicated configured root keys.
    pub roots_digest: String,
    pub roots_coverage: RepositoryRootsCoverage,
    pub omitted_root_count: u32,
    pub roots: Vec<RepositoryRootObservation>,
}

/// Whether every configured root has an observation, not whether each one succeeded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryRootsCoverage {
    Complete,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryRootObservation {
    /// The configured root spelling, used as the storage key.
    pub project_root: String,
    pub git_worktree_root: Option<String>,
    pub head: RepositoryHead,
    pub worktree: RepositoryWorktree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryHead {
    /// A full SHA-1 or SHA-256 object ID; a detached HEAD has no symbolic ref.
    Commit {
        oid: String,
        head_ref: Option<String>,
    },
    Unborn {
        head_ref: Option<String>,
    },
    Unknown {
        reason: RepositoryUnknownReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryWorktree {
    Clean,
    Dirty { coverage: RepositoryDirtyCoverage },
    Unknown { reason: RepositoryUnknownReason },
}

/// Coverage of dirty content. A digest exists only when coverage is complete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryDirtyCoverage {
    Complete { digest: String },
    Unknown { reason: RepositoryUnknownReason },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RepositoryUnknownReason {
    NotGit,
    MissingRoot,
    InaccessibleRoot,
    GitUnavailable,
    Timeout,
    OutputLimit,
    UnsupportedPath,
    UnstableSample,
    DirtyFingerprintNotCollected,
}

impl RepositoryUnknownReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NotGit => "notGit",
            Self::MissingRoot => "missingRoot",
            Self::InaccessibleRoot => "inaccessibleRoot",
            Self::GitUnavailable => "gitUnavailable",
            Self::Timeout => "timeout",
            Self::OutputLimit => "outputLimit",
            Self::UnsupportedPath => "unsupportedPath",
            Self::UnstableSample => "unstableSample",
            Self::DirtyFingerprintNotCollected => "dirtyFingerprintNotCollected",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        [
            Self::NotGit,
            Self::MissingRoot,
            Self::InaccessibleRoot,
            Self::GitUnavailable,
            Self::Timeout,
            Self::OutputLimit,
            Self::UnsupportedPath,
            Self::UnstableSample,
            Self::DirtyFingerprintNotCollected,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == value)
    }
}

impl RepositoryObservation {
    pub(crate) fn validate(&self) -> Result<(), RepositoryObservationError> {
        if !is_bounded_text(&self.project_id, MAX_PROJECT_ID_BYTES) {
            return Err(RepositoryObservationError::InvalidProjectId);
        }
        if self.completed_at_ms < self.started_at_ms {
            return Err(RepositoryObservationError::InvalidInterval);
        }
        if !is_bounded_text(&self.roots_digest, MAX_DIGEST_BYTES) {
            return Err(RepositoryObservationError::InvalidDigest);
        }
        if self.roots_coverage == RepositoryRootsCoverage::Complete && self.omitted_root_count != 0
        {
            return Err(RepositoryObservationError::OmittedRootsWithCompleteCoverage);
        }
        if self.roots.len() > MAX_OBSERVED_ROOTS {
            return Err(RepositoryObservationError::TooManyRoots);
        }
        let mut seen = HashSet::new();
        for root in &self.roots {
            if !seen.insert(root.project_root.as_str()) {
                return Err(RepositoryObservationError::DuplicateRoot(
                    root.project_root.clone(),
                ));
            }
            root.validate()?;
        }
        Ok(())
    }
}

impl RepositoryRootObservation {
    fn validate(&self) -> Result<(), RepositoryObservationError> {
        let invalid_path = || RepositoryObservationError::InvalidPath(self.project_root.clone());
        if !is_bounded_text(&self.project_root, MAX_PATH_BYTES) {
            return Err(invalid_path());
        }
        if let Some(worktree_root) = &self.git_worktree_root
            && !is_bounded_text(worktree_root, MAX_PATH_BYTES)
        {
            return Err(invalid_path());
        }
        let head_ref = match &self.head {
            RepositoryHead::Commit { oid, head_ref } => {
                if !is_object_id(oid) {
                    return Err(RepositoryObservationError::InvalidObjectId(oid.clone()));
                }
                head_ref.as_deref()
            }
            RepositoryHead::Unborn { head_ref } => head_ref.as_deref(),
            RepositoryHead::Unknown { .. } => None,
        };
        if let Some(head_ref) = head_ref
            && !is_bounded_text(head_ref, MAX_REF_BYTES)
        {
            return Err(RepositoryObservationError::InvalidRef(head_ref.to_string()));
        }
        if let RepositoryWorktree::Dirty {
            coverage: RepositoryDirtyCoverage::Complete { digest },
        } = &self.worktree
            && !is_bounded_text(digest, MAX_DIGEST_BYTES)
        {
            return Err(RepositoryObservationError::InvalidDigest);
        }
        self.unknown_reason().map(|_| ())
    }

    /// The single stored reason for this root. Every unknown component must agree.
    pub(crate) fn unknown_reason(
        &self,
    ) -> Result<Option<RepositoryUnknownReason>, RepositoryObservationError> {
        let head = match &self.head {
            RepositoryHead::Unknown { reason } => Some(*reason),
            RepositoryHead::Commit { .. } | RepositoryHead::Unborn { .. } => None,
        };
        let worktree = match &self.worktree {
            RepositoryWorktree::Unknown { reason }
            | RepositoryWorktree::Dirty {
                coverage: RepositoryDirtyCoverage::Unknown { reason },
            } => Some(*reason),
            RepositoryWorktree::Clean
            | RepositoryWorktree::Dirty {
                coverage: RepositoryDirtyCoverage::Complete { .. },
            } => None,
        };
        match (head, worktree) {
            (Some(head), Some(worktree)) if head != worktree => Err(
                RepositoryObservationError::ConflictingUnknownReasons(self.project_root.clone()),
            ),
            (head, worktree) => Ok(head.or(worktree)),
        }
    }
}

fn is_bounded_text(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= maximum_bytes && !value.chars().any(char::is_control)
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RepositoryObservationError {
    #[error("repository observation ID must be non-empty, bounded, and contain no controls")]
    InvalidId,
    #[error("project ID must be non-empty, bounded, and contain no controls")]
    InvalidProjectId,
    #[error("repository observation must complete no earlier than it started")]
    InvalidInterval,
    #[error("repository observation digest must be non-empty, bounded, and contain no controls")]
    InvalidDigest,
    #[error("complete root coverage cannot omit roots")]
    OmittedRootsWithCompleteCoverage,
    #[error("repository observation has more than {MAX_OBSERVED_ROOTS} roots")]
    TooManyRoots,
    #[error("repository root was observed more than once: {0}")]
    DuplicateRoot(String),
    #[error("repository observation path is invalid for root: {0}")]
    InvalidPath(String),
    #[error("HEAD object ID must be a full lowercase SHA-1 or SHA-256 hex ID: {0}")]
    InvalidObjectId(String),
    #[error("HEAD ref must be non-empty, bounded, and contain no controls: {0}")]
    InvalidRef(String),
    #[error("unknown HEAD and working-tree reasons must agree for root: {0}")]
    ConflictingUnknownReasons(String),
}
