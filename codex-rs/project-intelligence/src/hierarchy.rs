use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

const MAX_PROJECT_ID_BYTES: usize = 512;
const MAX_NODE_ID_BYTES: usize = 512;
const MAX_RELATIVE_PATH_BYTES: usize = 4_096;
const MAX_PATH_SEGMENT_BYTES: usize = 255;
const MAX_ROOT_BYTES: usize = 32_768;
const MAX_ANCHOR_SCHEME_BYTES: usize = 64;
const MAX_ANCHOR_LOCATOR_BYTES: usize = 4_096;
const MAX_SOURCE_FINGERPRINT_BYTES: usize = 512;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HierarchyNodeId(String);

impl HierarchyNodeId {
    pub fn parse(value: impl Into<String>) -> Result<Self, HierarchyError> {
        let value = value.into();
        validate_identity("node ID", &value, MAX_NODE_ID_BYTES)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HierarchyNodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Logical path below one selected project root.
///
/// Paths use `/` on every host. The empty path represents the selected root
/// itself. Callers convert local filesystem paths at the boundary.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProjectRelativePath(String);

impl ProjectRelativePath {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, HierarchyError> {
        let value = value.into();
        if value.is_empty() {
            return Ok(Self::root());
        }
        if value.len() > MAX_RELATIVE_PATH_BYTES {
            return Err(HierarchyError::RelativePathTooLong);
        }
        if value.starts_with('/') || value.ends_with('/') || value.contains('\\') {
            return Err(HierarchyError::InvalidRelativePath(value));
        }
        for segment in value.split('/') {
            if segment.is_empty()
                || matches!(segment, "." | "..")
                || segment.contains('\0')
                || segment.len() > MAX_PATH_SEGMENT_BYTES
            {
                return Err(HierarchyError::InvalidRelativePath(value));
            }
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for ProjectRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Opaque, bounded identity for the exact source bytes represented by a node.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SourceFingerprint(String);

impl SourceFingerprint {
    pub fn parse(value: impl Into<String>) -> Result<Self, HierarchyError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_SOURCE_FINGERPRINT_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(HierarchyError::InvalidSourceFingerprint);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Project,
    Directory,
    File,
    Region,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeLifecycle {
    Active,
    Missing,
    Replaced,
}

/// Domain-neutral locator for one bounded unit inside a file.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionAnchor {
    pub scheme: String,
    pub locator: String,
}

impl RegionAnchor {
    pub fn new(
        scheme: impl Into<String>,
        locator: impl Into<String>,
    ) -> Result<Self, HierarchyError> {
        let scheme = scheme.into();
        let locator = locator.into();
        if scheme.is_empty()
            || scheme.len() > MAX_ANCHOR_SCHEME_BYTES
            || !scheme.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
        {
            return Err(HierarchyError::InvalidAnchorScheme(scheme));
        }
        if locator.is_empty() || locator.len() > MAX_ANCHOR_LOCATOR_BYTES || locator.contains('\0')
        {
            return Err(HierarchyError::InvalidAnchorLocator);
        }
        Ok(Self { scheme, locator })
    }
}

/// Validated input for creating one hierarchy node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewHierarchyNode {
    pub project_id: String,
    pub parent_id: Option<HierarchyNodeId>,
    pub kind: NodeKind,
    /// Exact configured project-root identity for non-project nodes.
    pub project_root: Option<String>,
    pub relative_path: ProjectRelativePath,
    pub region_anchor: Option<RegionAnchor>,
    pub source_fingerprint: Option<SourceFingerprint>,
}

impl NewHierarchyNode {
    pub fn validate(&self) -> Result<(), HierarchyError> {
        validate_identity("project ID", &self.project_id, MAX_PROJECT_ID_BYTES)?;
        if self.project_root.as_ref().is_some_and(|root| {
            root.is_empty() || root.len() > MAX_ROOT_BYTES || root.contains('\0')
        }) {
            return Err(HierarchyError::InvalidProjectRoot);
        }
        match self.kind {
            NodeKind::Project => {
                if self.parent_id.is_some()
                    || self.project_root.is_some()
                    || !self.relative_path.is_root()
                    || self.region_anchor.is_some()
                    || self.source_fingerprint.is_some()
                {
                    return Err(HierarchyError::InvalidProjectNode);
                }
            }
            NodeKind::Directory => {
                if self.parent_id.is_none()
                    || self.project_root.is_none()
                    || self.region_anchor.is_some()
                {
                    return Err(HierarchyError::InvalidDirectoryNode);
                }
            }
            NodeKind::File => {
                if self.parent_id.is_none()
                    || self.project_root.is_none()
                    || self.relative_path.is_root()
                    || self.region_anchor.is_some()
                {
                    return Err(HierarchyError::InvalidFileNode);
                }
            }
            NodeKind::Region => {
                if self.parent_id.is_none()
                    || self.project_root.is_none()
                    || self.relative_path.is_root()
                    || self.region_anchor.is_none()
                {
                    return Err(HierarchyError::InvalidRegionNode);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HierarchyNode {
    pub id: HierarchyNodeId,
    pub value: NewHierarchyNode,
    pub lifecycle: NodeLifecycle,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HierarchyError {
    #[error("{0} must be non-empty, bounded, and contain no control characters")]
    InvalidIdentity(&'static str),
    #[error("project-relative path is invalid: {0}")]
    InvalidRelativePath(String),
    #[error("project-relative path exceeds the maximum length")]
    RelativePathTooLong,
    #[error("region anchor scheme is invalid: {0}")]
    InvalidAnchorScheme(String),
    #[error("region anchor locator must be non-empty, bounded, and contain no NUL")]
    InvalidAnchorLocator,
    #[error("project root must be non-empty, bounded, and contain no NUL")]
    InvalidProjectRoot,
    #[error("source fingerprint must be non-empty, bounded, and contain no control characters")]
    InvalidSourceFingerprint,
    #[error("project nodes cannot have a parent, root, path, anchor, or source fingerprint")]
    InvalidProjectNode,
    #[error("directory nodes require a parent and project root and cannot have an anchor")]
    InvalidDirectoryNode,
    #[error(
        "file nodes require a parent, project root, and non-root path and cannot have an anchor"
    )]
    InvalidFileNode,
    #[error("region nodes require a parent, project root, non-root path, and generic anchor")]
    InvalidRegionNode,
}

fn validate_identity(
    label: &'static str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), HierarchyError> {
    if value.is_empty() || value.len() > maximum_bytes || value.chars().any(char::is_control) {
        return Err(HierarchyError::InvalidIdentity(label));
    }
    Ok(())
}

#[cfg(test)]
#[path = "hierarchy_tests.rs"]
mod tests;
