use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::HierarchyNode;
use crate::HierarchyNodeId;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;
use crate::RegionAnchor;
use crate::SourceFingerprint;

const MAX_ID_BYTES: usize = 512;
const MAX_PROJECT_ID_BYTES: usize = 512;
const MAX_DESCRIPTION_BYTES: usize = 4_096;
const MAX_ROUTING_TERMS: usize = 64;
const MAX_ROUTING_TERM_BYTES: usize = 256;
const MAX_ROUTING_TERM_TOTAL_BYTES: usize = 4_096;
const MAX_QUERY_BYTES: usize = 1_024;
const MAX_QUERY_RESULTS: u32 = 20;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ContextMapEntryId(String);

impl ContextMapEntryId {
    pub fn parse(value: impl Into<String>) -> Result<Self, ContextMapError> {
        let value = value.into();
        validate_identity(&value, MAX_ID_BYTES).map_err(|()| ContextMapError::InvalidEntryId)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContextMapEntryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextMapCoverage {
    Complete,
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextMapFreshness {
    Current,
    Stale,
    SourceUnavailable,
}

/// Bounded routing metadata for one file or generic anchored region.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewContextMapEntry {
    pub project_id: String,
    pub node_id: HierarchyNodeId,
    pub source_fingerprint: SourceFingerprint,
    pub description: String,
    pub routing_terms: Vec<String>,
    pub coverage: ContextMapCoverage,
}

impl NewContextMapEntry {
    pub fn validate(&self) -> Result<(), ContextMapError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| ContextMapError::InvalidProjectId)?;
        if self.description.is_empty()
            || self.description.len() > MAX_DESCRIPTION_BYTES
            || self.description.contains('\0')
        {
            return Err(ContextMapError::InvalidDescription);
        }
        if self.routing_terms.len() > MAX_ROUTING_TERMS {
            return Err(ContextMapError::TooManyRoutingTerms);
        }
        let mut total_bytes = 0;
        let mut unique_terms = HashSet::with_capacity(self.routing_terms.len());
        for term in &self.routing_terms {
            total_bytes += term.len();
            if term.is_empty()
                || term.len() > MAX_ROUTING_TERM_BYTES
                || term.trim() != term
                || term.chars().any(char::is_control)
                || !unique_terms.insert(term)
            {
                return Err(ContextMapError::InvalidRoutingTerm(term.clone()));
            }
        }
        if total_bytes > MAX_ROUTING_TERM_TOTAL_BYTES {
            return Err(ContextMapError::RoutingTermsTooLarge);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapEntry {
    pub id: ContextMapEntryId,
    pub value: NewContextMapEntry,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_verified_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapQuery {
    pub project_id: String,
    pub text: String,
    pub max_results: u32,
}

impl ContextMapQuery {
    pub fn validate(&self) -> Result<(), ContextMapError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| ContextMapError::InvalidProjectId)?;
        if self.text.is_empty()
            || self.text.len() > MAX_QUERY_BYTES
            || self.text.trim() != self.text
            || self.max_results == 0
            || self.max_results > MAX_QUERY_RESULTS
        {
            return Err(ContextMapError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapQueryResult {
    pub data: Vec<ContextMapHit>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapListQuery {
    pub project_id: String,
    pub max_results: u32,
}

impl ContextMapListQuery {
    pub fn validate(&self) -> Result<(), ContextMapError> {
        validate_identity(&self.project_id, MAX_PROJECT_ID_BYTES)
            .map_err(|()| ContextMapError::InvalidProjectId)?;
        if self.max_results == 0 || self.max_results > MAX_QUERY_RESULTS {
            return Err(ContextMapError::InvalidQuery);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapHit {
    pub entry: ContextMapEntry,
    pub source: ContextMapSource,
    pub freshness: ContextMapFreshness,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapSource {
    pub project_root: String,
    pub relative_path: ProjectRelativePath,
    pub region_anchor: Option<RegionAnchor>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMapEntryUpdate {
    pub expected_revision: u64,
    pub source_fingerprint: SourceFingerprint,
    pub description: String,
    pub routing_terms: Vec<String>,
    pub coverage: ContextMapCoverage,
}

impl ContextMapEntry {
    pub fn freshness_against(
        &self,
        node: &HierarchyNode,
    ) -> Result<ContextMapFreshness, ContextMapError> {
        if self.value.project_id != node.value.project_id || self.value.node_id != node.id {
            return Err(ContextMapError::HierarchyBindingMismatch);
        }
        if !matches!(node.value.kind, NodeKind::File | NodeKind::Region) {
            return Err(ContextMapError::UnsupportedNodeKind(node.value.kind));
        }
        match node.lifecycle {
            NodeLifecycle::Missing => Ok(ContextMapFreshness::SourceUnavailable),
            NodeLifecycle::Replaced => Ok(ContextMapFreshness::Stale),
            NodeLifecycle::Active => {
                if node.value.source_fingerprint.as_ref() == Some(&self.value.source_fingerprint) {
                    Ok(ContextMapFreshness::Current)
                } else {
                    Ok(ContextMapFreshness::Stale)
                }
            }
        }
    }

    pub fn source_route(&self, node: &HierarchyNode) -> Result<ContextMapSource, ContextMapError> {
        self.freshness_against(node)?;
        Ok(ContextMapSource {
            project_root: node
                .value
                .project_root
                .clone()
                .ok_or(ContextMapError::MissingSourceRoute)?,
            relative_path: node.value.relative_path.clone(),
            region_anchor: node.value.region_anchor.clone(),
        })
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextMapError {
    #[error("context-map entry ID must be non-empty, bounded, and contain no controls")]
    InvalidEntryId,
    #[error("project ID must be non-empty, bounded, and contain no controls")]
    InvalidProjectId,
    #[error("context-map description must be non-empty, bounded, and contain no NUL")]
    InvalidDescription,
    #[error("context-map entry has too many routing terms")]
    TooManyRoutingTerms,
    #[error("context-map routing term is invalid or duplicated: {0}")]
    InvalidRoutingTerm(String),
    #[error("context-map routing terms exceed their combined size limit")]
    RoutingTermsTooLarge,
    #[error("context-map entry and hierarchy node do not describe the same source")]
    HierarchyBindingMismatch,
    #[error("context-map entries require a file or region node, found {0:?}")]
    UnsupportedNodeKind(NodeKind),
    #[error("context-map query must be non-empty, bounded, trimmed, and request 1-20 results")]
    InvalidQuery,
    #[error("context-map query contains no searchable terms")]
    NoSearchTerms,
    #[error("context-map hierarchy node is missing its exact source route")]
    MissingSourceRoute,
}

fn validate_identity(value: &str, maximum_bytes: usize) -> Result<(), ()> {
    if value.is_empty() || value.len() > maximum_bytes || value.chars().any(char::is_control) {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
#[path = "context_map_tests.rs"]
mod tests;
