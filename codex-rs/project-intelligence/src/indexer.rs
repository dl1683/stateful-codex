use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

use sha2::Digest;
use sha2::Sha256;
use thiserror::Error;

mod regions;
mod publish;
mod scan;

use publish::mark_file_missing;
use publish::publish_file;
use scan::normalized_relative_path;
use scan::scan_project_file;
use scan::scan_roots;

use crate::ContextMapStore;
use crate::ContextMapStoreError;
use crate::HierarchyNode;
use crate::HierarchyNodeId;
use crate::HierarchyStore;
use crate::HierarchyStoreError;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIndexRequest {
    pub project_id: String,
    pub roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIndexFileRequest {
    pub project_id: String,
    pub project_root: PathBuf,
    pub relative_path: ProjectRelativePath,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectIndexReport {
    pub files_indexed: u64,
    pub files_skipped: u64,
    pub missing_files: u64,
    pub truncated: bool,
}

#[derive(Clone)]
pub struct ProjectIndexer {
    hierarchy: HierarchyStore,
    context_map: ContextMapStore,
}

impl ProjectIndexer {
    pub fn new(hierarchy: HierarchyStore, context_map: ContextMapStore) -> Self {
        Self {
            hierarchy,
            context_map,
        }
    }

    pub async fn refresh(
        &self,
        request: ProjectIndexRequest,
    ) -> Result<ProjectIndexReport, ProjectIndexerError> {
        validate_request(&request)?;
        let project_id = request.project_id.clone();
        let roots = request.roots.clone();
        let scan = tokio::task::spawn_blocking(move || scan_roots(&roots))
            .await
            .map_err(ProjectIndexerError::ScanTask)??;
        let project_node_id = stable_id("project", &[&project_id])?;
        self.hierarchy
            .create_node(
                project_node_id.clone(),
                NewHierarchyNode {
                    project_id: project_id.clone(),
                    parent_id: None,
                    kind: NodeKind::Project,
                    project_root: None,
                    relative_path: ProjectRelativePath::root(),
                    region_anchor: None,
                    source_fingerprint: None,
                },
            )
            .await?;

        let mut root_nodes = HashMap::new();
        for root in &request.roots {
            let root_text = root.display().to_string();
            let root_id = stable_id("root", &[&project_id, &root_text])?;
            self.hierarchy
                .create_node(
                    root_id.clone(),
                    NewHierarchyNode {
                        project_id: project_id.clone(),
                        parent_id: Some(project_node_id.clone()),
                        kind: NodeKind::Directory,
                        project_root: Some(root_text.clone()),
                        relative_path: ProjectRelativePath::root(),
                        region_anchor: None,
                        source_fingerprint: None,
                    },
                )
                .await?;
            root_nodes.insert(root_text, root_id);
        }

        let mut directory_nodes = HashMap::new();
        let mut seen_files = HashSet::new();
        for file in scan.files {
            let root_id = root_nodes
                .get(&file.project_root)
                .ok_or(ProjectIndexerError::UnrecognizedRoot)?;
            let parent_id = self
                .ensure_parent_directories(
                    &project_id,
                    &file.project_root,
                    root_id,
                    &file.relative_path,
                    &mut directory_nodes,
                )
                .await?;
            let file_id = stable_id(
                "file",
                &[&project_id, &file.project_root, &file.relative_path],
            )?;
            publish_file(self, &project_id, &file_id, parent_id, &file).await?;
            seen_files.insert(file_id);
        }

        let mut report = ProjectIndexReport {
            files_indexed: u64::try_from(seen_files.len())
                .map_err(|_| ProjectIndexerError::CountOverflow)?,
            files_skipped: scan.files_skipped,
            missing_files: 0,
            truncated: scan.truncated,
        };
        if scan.inventory_complete {
            for root_id in root_nodes.values() {
                report.missing_files += self
                    .mark_missing_files(&project_id, root_id, &seen_files)
                    .await?;
            }
        }
        Ok(report)
    }

    pub async fn refresh_file(
        &self,
        request: ProjectIndexFileRequest,
    ) -> Result<ProjectIndexReport, ProjectIndexerError> {
        validate_file_request(&request)?;
        let project_id = request.project_id;
        let project_root = request.project_root;
        let relative_path = request.relative_path;
        let project_root_text = project_root.display().to_string();
        let file_id = stable_id(
            "file",
            &[&project_id, &project_root_text, relative_path.as_str()],
        )?;
        let scan_root = project_root.clone();
        let scan_path = relative_path.clone();
        let file = tokio::task::spawn_blocking(move || scan_project_file(&scan_root, &scan_path))
            .await
            .map_err(ProjectIndexerError::ScanTask)??;
        let Some(file) = file else {
            let existing = self
                .hierarchy
                .get_node(&project_id, &file_id)
                .await?
                .ok_or_else(|| ProjectIndexerError::SourceNotIndexed(relative_path.to_string()))?;
            if existing.value.kind != NodeKind::File
                || existing.value.project_root.as_deref() != Some(&project_root_text)
                || existing.value.relative_path != relative_path
            {
                return Err(ProjectIndexerError::IdentityConflict(file_id.to_string()));
            }
            let missing_files = if existing.lifecycle == NodeLifecycle::Active {
                self.mark_missing(&project_id, existing).await?;
                1
            } else {
                0
            };
            return Ok(ProjectIndexReport {
                files_indexed: 0,
                files_skipped: 0,
                missing_files,
                truncated: false,
            });
        };
        let project_node_id = stable_id("project", &[&project_id])?;
        self.hierarchy
            .create_node(
                project_node_id.clone(),
                NewHierarchyNode {
                    project_id: project_id.clone(),
                    parent_id: None,
                    kind: NodeKind::Project,
                    project_root: None,
                    relative_path: ProjectRelativePath::root(),
                    region_anchor: None,
                    source_fingerprint: None,
                },
            )
            .await?;
        let project_root = project_root_text;
        let root_id = stable_id("root", &[&project_id, &project_root])?;
        self.hierarchy
            .create_node(
                root_id.clone(),
                NewHierarchyNode {
                    project_id: project_id.clone(),
                    parent_id: Some(project_node_id),
                    kind: NodeKind::Directory,
                    project_root: Some(project_root.clone()),
                    relative_path: ProjectRelativePath::root(),
                    region_anchor: None,
                    source_fingerprint: None,
                },
            )
            .await?;
        let mut directory_nodes = HashMap::new();
        let parent_id = self
            .ensure_parent_directories(
                &project_id,
                &project_root,
                &root_id,
                relative_path.as_str(),
                &mut directory_nodes,
            )
            .await?;
        publish_file(self, &project_id, &file_id, parent_id, &file).await?;
        Ok(ProjectIndexReport {
            files_indexed: 1,
            files_skipped: 0,
            missing_files: 0,
            truncated: false,
        })
    }

    async fn ensure_parent_directories(
        &self,
        project_id: &str,
        project_root: &str,
        root_id: &HierarchyNodeId,
        relative_file: &str,
        cache: &mut HashMap<(String, String), HierarchyNodeId>,
    ) -> Result<HierarchyNodeId, ProjectIndexerError> {
        let path = Path::new(relative_file);
        let Some(parent) = path.parent() else {
            return Ok(root_id.clone());
        };
        let mut current = root_id.clone();
        let mut relative = PathBuf::new();
        for component in parent.components() {
            relative.push(component);
            let relative_text = normalized_relative_path(&relative)?;
            let cache_key = (project_root.to_string(), relative_text.clone());
            if let Some(id) = cache.get(&cache_key) {
                current = id.clone();
                continue;
            }
            let id = stable_id("directory", &[project_id, project_root, &relative_text])?;
            self.hierarchy
                .create_node(
                    id.clone(),
                    NewHierarchyNode {
                        project_id: project_id.to_string(),
                        parent_id: Some(current),
                        kind: NodeKind::Directory,
                        project_root: Some(project_root.to_string()),
                        relative_path: ProjectRelativePath::parse(&relative_text)?,
                        region_anchor: None,
                        source_fingerprint: None,
                    },
                )
                .await?;
            cache.insert(cache_key, id.clone());
            current = id;
        }
        Ok(current)
    }

    async fn mark_missing_files(
        &self,
        project_id: &str,
        root_id: &HierarchyNodeId,
        seen_files: &HashSet<HierarchyNodeId>,
    ) -> Result<u64, ProjectIndexerError> {
        let mut missing = 0_u64;
        let mut pending = vec![root_id.clone()];
        while let Some(parent_id) = pending.pop() {
            for child in self.hierarchy.list_children(project_id, &parent_id).await? {
                match child.value.kind {
                    NodeKind::Project => {}
                    NodeKind::Directory | NodeKind::Region => pending.push(child.id),
                    NodeKind::File => {
                        if child.lifecycle == NodeLifecycle::Active
                            && !seen_files.contains(&child.id)
                        {
                            self.mark_missing(project_id, child).await?;
                            missing = missing
                                .checked_add(1)
                                .ok_or(ProjectIndexerError::CountOverflow)?;
                        }
                    }
                }
            }
        }
        Ok(missing)
    }

    async fn mark_missing(
        &self,
        project_id: &str,
        file: HierarchyNode,
    ) -> Result<(), ProjectIndexerError> {
        mark_file_missing(self, project_id, &file.id).await
    }
}

fn validate_request(request: &ProjectIndexRequest) -> Result<(), ProjectIndexerError> {
    if request.project_id.is_empty() || request.roots.is_empty() {
        return Err(ProjectIndexerError::InvalidRequest);
    }
    if request.roots.iter().any(|root| !root.is_absolute()) {
        return Err(ProjectIndexerError::InvalidRoot);
    }
    Ok(())
}

fn validate_file_request(request: &ProjectIndexFileRequest) -> Result<(), ProjectIndexerError> {
    if request.project_id.is_empty() || request.relative_path.is_root() {
        return Err(ProjectIndexerError::InvalidRequest);
    }
    if !request.project_root.is_absolute() {
        return Err(ProjectIndexerError::InvalidRoot);
    }
    Ok(())
}

fn stable_id(prefix: &str, components: &[&str]) -> Result<HierarchyNodeId, ProjectIndexerError> {
    Ok(HierarchyNodeId::parse(stable_id_text(prefix, components))?)
}

fn stable_id_text(prefix: &str, components: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for component in components {
        hasher.update(component.as_bytes());
        hasher.update([0]);
    }
    format!("stateful-{prefix}-{}", hex_digest(hasher.finalize()))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[derive(Debug, Error)]
pub enum ProjectIndexerError {
    #[error("project index request requires a project ID and at least one absolute root")]
    InvalidRequest,
    #[error("project index roots must be absolute and contain only supported paths")]
    InvalidRoot,
    #[error("indexed file did not belong to a configured project root")]
    UnrecognizedRoot,
    #[error("source is not indexed in the selected project: {0}")]
    SourceNotIndexed(String),
    #[error("stable indexed identity conflicts with existing hierarchy: {0}")]
    IdentityConflict(String),
    #[error("project index count overflow")]
    CountOverflow,
    #[error(transparent)]
    Hierarchy(#[from] HierarchyStoreError),
    #[error(transparent)]
    ContextMap(#[from] ContextMapStoreError),
    #[error(transparent)]
    HierarchyValue(#[from] crate::HierarchyError),
    #[error(transparent)]
    ContextMapValue(#[from] crate::ContextMapError),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("project index scan task failed: {0}")]
    ScanTask(tokio::task::JoinError),
}

#[cfg(test)]
#[path = "indexer_tests.rs"]
mod tests;
