use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::ContextMapRegionAnchor as ApiContextMapRegionAnchor;
use codex_app_server_protocol::ContextMapSource as ApiContextMapSource;
use codex_app_server_protocol::EvidenceEncoding;
use codex_app_server_protocol::EvidenceReadParams;
use codex_app_server_protocol::EvidenceReadResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ProjectIntelligenceNode;
use codex_app_server_protocol::ProjectIntelligenceNodeKind;
use codex_app_server_protocol::ProjectIntelligenceNodeLifecycle;
use codex_app_server_protocol::ProjectIntelligenceStatusParams;
use codex_app_server_protocol::ProjectIntelligenceStatusResponse;
use codex_app_server_protocol::ProjectIntelligenceTreeParams;
use codex_app_server_protocol::ProjectIntelligenceTreeResponse;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::ContextMapStoreError;
use codex_project_intelligence::EvidenceLineRange as InternalEvidenceLineRange;
use codex_project_intelligence::EvidenceReadLocator as InternalEvidenceReadLocator;
use codex_project_intelligence::EvidenceReadRequest as InternalEvidenceReadRequest;
use codex_project_intelligence::EvidenceReader;
use codex_project_intelligence::EvidenceRoute;
use codex_project_intelligence::HierarchyNode;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::NodeKind;
use codex_project_intelligence::NodeLifecycle;
use codex_state::SqliteConfig;
use codex_thread_store::StoredProject;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use codex_utils_absolute_path::AbsolutePathBuf;
use sha2::Digest;
use sha2::Sha256;
use tokio::io::AsyncReadExt;
use tokio::sync::OnceCell;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::method_not_found;

const DEFAULT_EVIDENCE_BYTES: u32 = 32 * 1024;
const MAX_EVIDENCE_BYTES: u32 = 64 * 1024;
const DEFAULT_TREE_PAGE_SIZE: u32 = 200;
const MAX_TREE_PAGE_SIZE: u32 = 500;

#[derive(Clone)]
pub(crate) struct ProjectIntelligenceRequestProcessor {
    thread_store: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
    hierarchy: Arc<OnceCell<HierarchyStore>>,
    context_map: Arc<OnceCell<ContextMapStore>>,
}

impl ProjectIntelligenceRequestProcessor {
    pub(crate) fn new(thread_store: Arc<dyn ThreadStore>, sqlite: Option<SqliteConfig>) -> Self {
        Self {
            thread_store,
            sqlite,
            hierarchy: Arc::new(OnceCell::new()),
            context_map: Arc::new(OnceCell::new()),
        }
    }

    pub(crate) async fn status(
        &self,
        params: ProjectIntelligenceStatusParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project = self.project(&params.project_id).await?;
        let status = self
            .hierarchy()
            .await?
            .project_intelligence_status(&params.project_id)
            .await
            .map_err(|error| internal_error(format!("failed to read project status: {error}")))?;
        let roots = project
            .roots
            .into_iter()
            .map(|root| {
                AbsolutePathBuf::from_absolute_path(PathBuf::from(root.path)).map_err(|error| {
                    internal_error(format!("stored project root is not absolute: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(
            ProjectIntelligenceStatusResponse {
                project_id: params.project_id,
                roots,
                initialized: status.initialized,
                revision: status.revision,
                hierarchy_node_count: status.hierarchy_node_count,
                file_count: status.file_count,
                missing_source_count: status.missing_source_count,
                context_map_entry_count: status.context_map_entry_count,
                blackboard_entry_count: status.blackboard_entry_count,
                promoted_entry_count: status.promoted_entry_count,
                updated_at: status
                    .updated_at_ms
                    .map(|timestamp| timestamp.div_euclid(/*rhs*/ 1000)),
            }
            .into(),
        ))
    }

    pub(crate) async fn evidence_read(
        &self,
        params: EvidenceReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let project = self.project(&params.project_id).await?;
        let max_bytes = params.max_bytes.unwrap_or(DEFAULT_EVIDENCE_BYTES);
        if max_bytes == 0 || max_bytes > MAX_EVIDENCE_BYTES {
            return Err(invalid_params("maxBytes must be between 1 and 65536"));
        }
        let entry_id = ContextMapEntryId::parse(params.context_map_entry_id)
            .map_err(|error| invalid_params(error.to_string()))?;
        let hit = self
            .context_map()
            .await?
            .get_hit(&params.project_id, &entry_id)
            .await
            .map_err(context_map_error)?
            .ok_or_else(|| invalid_params(format!("context-map entry not found: {entry_id}")))?;
        if hit.freshness != ContextMapFreshness::Current {
            return Err(invalid_params(format!(
                "evidence source is {:?}; refresh and verify it before reading",
                hit.freshness
            )));
        }
        if !project
            .roots
            .iter()
            .any(|root| root.path == hit.source.project_root)
        {
            return Err(internal_error(
                "stored evidence root is outside the selected project",
            ));
        }
        let root = tokio::fs::canonicalize(&hit.source.project_root)
            .await
            .map_err(|error| invalid_params(format!("evidence root is unavailable: {error}")))?;
        let path = root.join(hit.source.relative_path.as_str());
        let canonical_path = tokio::fs::canonicalize(&path)
            .await
            .map_err(|error| invalid_params(format!("evidence source is unavailable: {error}")))?;
        if !canonical_path.starts_with(&root) {
            return Err(invalid_params(
                "evidence source resolves outside its project root",
            ));
        }
        if hit.source.region_anchor.is_some() {
            let route =
                EvidenceRoute::from_hit(&hit).map_err(|error| invalid_params(error.to_string()))?;
            let anchored_range = route.line_range.ok_or_else(|| {
                invalid_params("anchored-region evidence route has no exact line range")
            })?;
            if let Some(line_range) = params.line_range {
                let requested_range = InternalEvidenceLineRange {
                    start: line_range.start,
                    end: line_range.end,
                };
                if requested_range != anchored_range {
                    return Err(invalid_params(format!(
                        "lineRange must match the current anchored region {}-{}",
                        anchored_range.start, anchored_range.end
                    )));
                }
            }
            let read = EvidenceReader::new(self.context_map().await?.clone())
                .read(InternalEvidenceReadRequest {
                    project_id: params.project_id.clone(),
                    project_roots: project
                        .roots
                        .iter()
                        .map(|root| PathBuf::from(&root.path))
                        .collect(),
                    locator: InternalEvidenceReadLocator::ContextMapRoute(route),
                    max_bytes,
                })
                .await
                .map_err(|error| invalid_params(error.to_string()))?;
            return Ok(Some(
                EvidenceReadResponse {
                    project_id: params.project_id,
                    context_map_entry_id: entry_id.to_string(),
                    node_id: read.hit.entry.value.node_id.to_string(),
                    source_fingerprint: read.hit.entry.value.source_fingerprint.to_string(),
                    source: ApiContextMapSource {
                        project_root: AbsolutePathBuf::from_absolute_path(root).map_err(
                            |error| {
                                internal_error(format!(
                                    "canonical evidence root is invalid: {error}"
                                ))
                            },
                        )?,
                        relative_path: read.hit.source.relative_path.to_string(),
                        region_anchor: read.hit.source.region_anchor.map(|anchor| {
                            ApiContextMapRegionAnchor {
                                scheme: anchor.scheme,
                                locator: anchor.locator,
                            }
                        }),
                    },
                    encoding: EvidenceEncoding::Utf8,
                    content: read.content,
                    bytes_returned: read.bytes_returned,
                    total_bytes: read.total_bytes,
                    total_lines: read.total_lines,
                    first_line: read.first_line,
                    last_line: read.last_line,
                    truncated: read.truncated,
                    revision: read.hit.entry.revision,
                }
                .into(),
            ));
        }
        if let Some(line_range) = params.line_range {
            let read = EvidenceReader::new(self.context_map().await?.clone())
                .read(InternalEvidenceReadRequest {
                    project_id: params.project_id.clone(),
                    project_roots: project
                        .roots
                        .iter()
                        .map(|root| PathBuf::from(&root.path))
                        .collect(),
                    locator: InternalEvidenceReadLocator::Source {
                        project_root: Some(PathBuf::from(&hit.source.project_root)),
                        relative_path: hit.source.relative_path.clone(),
                        line_range: Some(InternalEvidenceLineRange {
                            start: line_range.start,
                            end: line_range.end,
                        }),
                    },
                    max_bytes,
                })
                .await
                .map_err(|error| invalid_params(error.to_string()))?;
            return Ok(Some(
                EvidenceReadResponse {
                    project_id: params.project_id,
                    context_map_entry_id: entry_id.to_string(),
                    node_id: read.hit.entry.value.node_id.to_string(),
                    source_fingerprint: read.hit.entry.value.source_fingerprint.to_string(),
                    source: ApiContextMapSource {
                        project_root: AbsolutePathBuf::from_absolute_path(root).map_err(
                            |error| {
                                internal_error(format!(
                                    "canonical evidence root is invalid: {error}"
                                ))
                            },
                        )?,
                        relative_path: read.hit.source.relative_path.to_string(),
                        region_anchor: None,
                    },
                    encoding: EvidenceEncoding::Utf8,
                    content: read.content,
                    bytes_returned: read.bytes_returned,
                    total_bytes: read.total_bytes,
                    total_lines: read.total_lines,
                    first_line: read.first_line,
                    last_line: read.last_line,
                    truncated: read.truncated,
                    revision: read.hit.entry.revision,
                }
                .into(),
            ));
        }
        let (bytes, total_bytes, total_lines, fingerprint) =
            read_and_fingerprint(&canonical_path, max_bytes).await?;
        if fingerprint != hit.entry.value.source_fingerprint.as_str() {
            return Err(invalid_params(
                "evidence source changed after indexing; refresh the context map",
            ));
        }
        let bytes_returned = u64::try_from(bytes.len())
            .map_err(|_| internal_error("evidence byte count overflow"))?;
        let (encoding, content) = match String::from_utf8(bytes.clone()) {
            Ok(content) => (EvidenceEncoding::Utf8, content),
            Err(_) => (EvidenceEncoding::Base64, STANDARD.encode(bytes)),
        };
        Ok(Some(
            EvidenceReadResponse {
                project_id: params.project_id,
                context_map_entry_id: entry_id.to_string(),
                node_id: hit.entry.value.node_id.to_string(),
                source_fingerprint: fingerprint,
                source: ApiContextMapSource {
                    project_root: AbsolutePathBuf::from_absolute_path(root).map_err(|error| {
                        internal_error(format!("canonical evidence root is invalid: {error}"))
                    })?,
                    relative_path: hit.source.relative_path.to_string(),
                    region_anchor: None,
                },
                encoding,
                content,
                bytes_returned,
                total_bytes,
                total_lines,
                first_line: None,
                last_line: None,
                truncated: bytes_returned < total_bytes,
                revision: hit.entry.revision,
            }
            .into(),
        ))
    }

    pub(crate) async fn tree(
        &self,
        params: ProjectIntelligenceTreeParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.project(&params.project_id).await?;
        let offset = params
            .cursor
            .as_deref()
            .map(str::parse::<u32>)
            .transpose()
            .map_err(|_| invalid_params("invalid project-intelligence tree cursor"))?
            .unwrap_or_default();
        let limit = params.limit.unwrap_or(DEFAULT_TREE_PAGE_SIZE);
        if limit == 0 || limit > MAX_TREE_PAGE_SIZE {
            return Err(invalid_params("limit must be between 1 and 500"));
        }
        let mut nodes = self
            .hierarchy()
            .await?
            .list_project_nodes(&params.project_id, offset, limit.saturating_add(1))
            .await
            .map_err(|error| {
                internal_error(format!("failed to read project hierarchy: {error}"))
            })?;
        let has_more = nodes.len() > limit as usize;
        nodes.truncate(limit as usize);
        let next_cursor = has_more.then(|| offset.saturating_add(limit).to_string());
        let data = nodes
            .into_iter()
            .map(api_node)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(
            ProjectIntelligenceTreeResponse { data, next_cursor }.into(),
        ))
    }

    async fn project(&self, project_id: &str) -> Result<StoredProject, JSONRPCErrorError> {
        self.thread_store
            .read_project(project_id.to_string())
            .await
            .map_err(project_error)?
            .ok_or_else(|| invalid_params(format!("project not found: {project_id}")))
    }

    async fn hierarchy(&self) -> Result<&HierarchyStore, JSONRPCErrorError> {
        let sqlite = self.sqlite.as_ref().ok_or_else(|| {
            method_not_found("projectIntelligence/status is unavailable without sqlite state")
        })?;
        self.hierarchy
            .get_or_try_init(|| HierarchyStore::open(sqlite))
            .await
            .map_err(|error| internal_error(format!("failed to open project hierarchy: {error}")))
    }

    async fn context_map(&self) -> Result<&ContextMapStore, JSONRPCErrorError> {
        let sqlite = self
            .sqlite
            .as_ref()
            .ok_or_else(|| method_not_found("evidence/read is unavailable without sqlite state"))?;
        self.context_map
            .get_or_try_init(|| ContextMapStore::open(sqlite))
            .await
            .map_err(context_map_error)
    }
}

fn api_node(node: HierarchyNode) -> Result<ProjectIntelligenceNode, JSONRPCErrorError> {
    Ok(ProjectIntelligenceNode {
        id: node.id.to_string(),
        parent_id: node.value.parent_id.map(|parent| parent.to_string()),
        kind: match node.value.kind {
            NodeKind::Project => ProjectIntelligenceNodeKind::Project,
            NodeKind::Directory => ProjectIntelligenceNodeKind::Directory,
            NodeKind::File => ProjectIntelligenceNodeKind::File,
            NodeKind::Region => ProjectIntelligenceNodeKind::Region,
        },
        project_root: node
            .value
            .project_root
            .map(|root| {
                AbsolutePathBuf::from_absolute_path(PathBuf::from(root)).map_err(|error| {
                    internal_error(format!("stored hierarchy root is not absolute: {error}"))
                })
            })
            .transpose()?,
        relative_path: node.value.relative_path.to_string(),
        region_anchor: node.value.region_anchor.map(|anchor| {
            codex_app_server_protocol::ContextMapRegionAnchor {
                scheme: anchor.scheme,
                locator: anchor.locator,
            }
        }),
        source_fingerprint: node
            .value
            .source_fingerprint
            .map(|fingerprint| fingerprint.to_string()),
        lifecycle: match node.lifecycle {
            NodeLifecycle::Active => ProjectIntelligenceNodeLifecycle::Active,
            NodeLifecycle::Missing => ProjectIntelligenceNodeLifecycle::Missing,
            NodeLifecycle::Replaced => ProjectIntelligenceNodeLifecycle::Replaced,
        },
        revision: node.revision,
        updated_at: node.updated_at_ms.div_euclid(/*rhs*/ 1000),
    })
}

async fn read_and_fingerprint(
    path: &std::path::Path,
    max_bytes: u32,
) -> Result<(Vec<u8>, u64, u64, String), JSONRPCErrorError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| invalid_params(format!("failed to open evidence source: {error}")))?;
    let mut hasher = Sha256::new();
    let mut returned = Vec::with_capacity(max_bytes as usize);
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;
    let mut total_lines = 0_u64;
    let mut saw_bytes = false;
    let mut ended_with_newline = false;
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| invalid_params(format!("failed to read evidence source: {error}")))?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes
            .checked_add(
                u64::try_from(read).map_err(|_| internal_error("evidence byte count overflow"))?,
            )
            .ok_or_else(|| internal_error("evidence byte count overflow"))?;
        hasher.update(&buffer[..read]);
        saw_bytes = true;
        total_lines = total_lines
            .checked_add(
                u64::try_from(buffer[..read].iter().filter(|byte| **byte == b'\n').count())
                    .map_err(|_| internal_error("evidence line count overflow"))?,
            )
            .ok_or_else(|| internal_error("evidence line count overflow"))?;
        ended_with_newline = buffer[read - 1] == b'\n';
        let remaining = max_bytes as usize - returned.len();
        returned.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    if saw_bytes && !ended_with_newline {
        total_lines = total_lines
            .checked_add(1)
            .ok_or_else(|| internal_error("evidence line count overflow"))?;
    }
    Ok((
        returned,
        total_bytes,
        total_lines,
        format!("sha256:{:x}", hasher.finalize()),
    ))
}

fn project_error(error: ThreadStoreError) -> JSONRPCErrorError {
    match error {
        ThreadStoreError::Unsupported { .. } => {
            method_not_found("project intelligence is unavailable without sqlite state")
        }
        ThreadStoreError::InvalidRequest { message } => invalid_params(message),
        error => internal_error(format!("failed to read project intelligence: {error}")),
    }
}

fn context_map_error(error: ContextMapStoreError) -> JSONRPCErrorError {
    match error {
        ContextMapStoreError::InvalidEntry(error) => invalid_params(error.to_string()),
        error => internal_error(format!("failed to read evidence route: {error}")),
    }
}
