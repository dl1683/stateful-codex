use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use sha2::Digest;
use sha2::Sha256;
use thiserror::Error;

mod generation;
mod publish;
mod regions;
mod scan;

use generation::RefreshGeneration;
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
use crate::storage::create_node_in_transaction;
use crate::storage::unix_timestamp_millis;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicationFence {
    Targeted,
    FullRefresh(RefreshGeneration),
}

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
    pub inventory_complete: bool,
    pub region_coverage_complete: bool,
    pub files_indexed: u64,
    pub regions_indexed: u64,
    pub files_skipped: u64,
    pub missing_files: u64,
    pub truncated: bool,
    pub scan_duration_ms: u64,
    pub publication_duration_ms: u64,
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
        let generation = generation::claim(self, &project_id).await?;
        let roots = request.roots.clone();
        let scan_started = Instant::now();
        let scan = tokio::task::spawn_blocking(move || scan_roots(&roots))
            .await
            .map_err(ProjectIndexerError::ScanTask)??;
        let scan_duration_ms = elapsed_millis(scan_started);
        let regions_indexed = scan.files.iter().try_fold(0_u64, |count, file| {
            count
                .checked_add(
                    u64::try_from(file.regions.len())
                        .map_err(|_| ProjectIndexerError::CountOverflow)?,
                )
                .ok_or(ProjectIndexerError::CountOverflow)
        })?;
        let publication_started = Instant::now();
        let mut report = self.publish_refresh(request, scan, generation).await?;
        report.regions_indexed = regions_indexed;
        report.scan_duration_ms = scan_duration_ms;
        report.publication_duration_ms = elapsed_millis(publication_started);
        self.record_refresh_status(&project_id, &report, generation)
            .await?;
        Ok(report)
    }

    async fn publish_refresh(
        &self,
        request: ProjectIndexRequest,
        scan: scan::ScanResult,
        generation: RefreshGeneration,
    ) -> Result<ProjectIndexReport, ProjectIndexerError> {
        let project_id = request.project_id.clone();
        let mut generation_check = self.context_map.begin_immediate().await?;
        generation::require_current(&mut generation_check, &project_id, generation).await?;
        generation_check.commit().await?;
        let inventory_complete = scan.inventory_complete;
        let region_coverage_complete = scan
            .files
            .iter()
            .all(|file| file.coverage == crate::ContextMapCoverage::Complete);
        let project_node_id = stable_id("project", &[&project_id])?;
        self.create_node(
            PublicationFence::FullRefresh(generation),
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
            self.create_node(
                PublicationFence::FullRefresh(generation),
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
                    PublicationFence::FullRefresh(generation),
                )
                .await?;
            let file_id = stable_id(
                "file",
                &[&project_id, &file.project_root, &file.relative_path],
            )?;
            publish_file(
                self,
                &project_id,
                &file_id,
                parent_id,
                &file,
                PublicationFence::FullRefresh(generation),
            )
            .await?;
            seen_files.insert(file_id);
        }

        let mut report = ProjectIndexReport {
            inventory_complete,
            region_coverage_complete,
            files_indexed: u64::try_from(seen_files.len())
                .map_err(|_| ProjectIndexerError::CountOverflow)?,
            regions_indexed: 0,
            files_skipped: scan.files_skipped,
            missing_files: 0,
            truncated: scan.truncated,
            scan_duration_ms: 0,
            publication_duration_ms: 0,
        };
        if scan.inventory_complete {
            for root_id in root_nodes.values() {
                report.missing_files += self
                    .mark_missing_files(&project_id, root_id, &seen_files, generation)
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
        let scan_started = Instant::now();
        let file = tokio::task::spawn_blocking(move || scan_project_file(&scan_root, &scan_path))
            .await
            .map_err(ProjectIndexerError::ScanTask)??;
        let scan_duration_ms = elapsed_millis(scan_started);
        let publication_started = Instant::now();
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
                self.mark_missing(&project_id, existing, PublicationFence::Targeted)
                    .await?;
                1
            } else {
                0
            };
            return Ok(ProjectIndexReport {
                inventory_complete: true,
                region_coverage_complete: true,
                files_indexed: 0,
                regions_indexed: 0,
                files_skipped: 0,
                missing_files,
                truncated: false,
                scan_duration_ms,
                publication_duration_ms: elapsed_millis(publication_started),
            });
        };
        let regions_indexed =
            u64::try_from(file.regions.len()).map_err(|_| ProjectIndexerError::CountOverflow)?;
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
                PublicationFence::Targeted,
            )
            .await?;
        publish_file(
            self,
            &project_id,
            &file_id,
            parent_id,
            &file,
            PublicationFence::Targeted,
        )
        .await?;
        Ok(ProjectIndexReport {
            inventory_complete: true,
            region_coverage_complete: file.coverage == crate::ContextMapCoverage::Complete,
            files_indexed: 1,
            regions_indexed,
            files_skipped: 0,
            missing_files: 0,
            truncated: false,
            scan_duration_ms,
            publication_duration_ms: elapsed_millis(publication_started),
        })
    }

    async fn record_refresh_status(
        &self,
        project_id: &str,
        report: &ProjectIndexReport,
        generation: RefreshGeneration,
    ) -> Result<(), ProjectIndexerError> {
        let count =
            |value: u64| i64::try_from(value).map_err(|_| ProjectIndexerError::CountOverflow);
        let mut transaction = self.context_map.begin_immediate().await?;
        generation::require_current(&mut transaction, project_id, generation).await?;
        sqlx::query(
            "INSERT INTO project_index_refresh_status (
                project_id, inventory_complete, region_coverage_complete,
                files_indexed, regions_indexed, files_skipped, missing_files,
                truncated, scan_duration_ms, publication_duration_ms, completed_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(project_id) DO UPDATE SET
                inventory_complete = excluded.inventory_complete,
                region_coverage_complete = excluded.region_coverage_complete,
                files_indexed = excluded.files_indexed,
                regions_indexed = excluded.regions_indexed,
                files_skipped = excluded.files_skipped,
                missing_files = excluded.missing_files,
                truncated = excluded.truncated,
                scan_duration_ms = excluded.scan_duration_ms,
                publication_duration_ms = excluded.publication_duration_ms,
                completed_at_ms = excluded.completed_at_ms",
        )
        .bind(project_id)
        .bind(report.inventory_complete)
        .bind(report.region_coverage_complete)
        .bind(count(report.files_indexed)?)
        .bind(count(report.regions_indexed)?)
        .bind(count(report.files_skipped)?)
        .bind(count(report.missing_files)?)
        .bind(report.truncated)
        .bind(count(report.scan_duration_ms)?)
        .bind(count(report.publication_duration_ms)?)
        .bind(unix_timestamp_millis()?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn create_node(
        &self,
        fence: PublicationFence,
        id: HierarchyNodeId,
        value: NewHierarchyNode,
    ) -> Result<HierarchyNode, ProjectIndexerError> {
        match fence {
            PublicationFence::Targeted => Ok(self.hierarchy.create_node(id, value).await?),
            PublicationFence::FullRefresh(generation) => {
                let project_id = value.project_id.clone();
                let mut transaction = self.context_map.begin_immediate().await?;
                generation::require_current(&mut transaction, &project_id, generation).await?;
                let node = create_node_in_transaction(&mut transaction, &id, value).await?;
                transaction.commit().await?;
                Ok(node)
            }
        }
    }

    async fn ensure_parent_directories(
        &self,
        project_id: &str,
        project_root: &str,
        root_id: &HierarchyNodeId,
        relative_file: &str,
        cache: &mut HashMap<(String, String), HierarchyNodeId>,
        fence: PublicationFence,
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
            self.create_node(
                fence,
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
        generation: RefreshGeneration,
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
                            self.mark_missing(
                                project_id,
                                child,
                                PublicationFence::FullRefresh(generation),
                            )
                            .await?;
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
        fence: PublicationFence,
    ) -> Result<(), ProjectIndexerError> {
        mark_file_missing(self, project_id, &file.id, fence).await
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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
    #[error("document extractor produced an invalid indexed identity")]
    InvalidExtractionIdentity,
    #[error("project refresh was superseded by a newer generation")]
    SupersededRefresh,
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
