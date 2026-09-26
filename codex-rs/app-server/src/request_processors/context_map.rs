use std::path::PathBuf;
use std::sync::Arc;

use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::ContextMapCoverage as ApiContextMapCoverage;
use codex_app_server_protocol::ContextMapFreshness as ApiContextMapFreshness;
use codex_app_server_protocol::ContextMapQueryHit as ApiContextMapQueryHit;
use codex_app_server_protocol::ContextMapQueryParams;
use codex_app_server_protocol::ContextMapQueryResponse;
use codex_app_server_protocol::ContextMapRefreshParams;
use codex_app_server_protocol::ContextMapRefreshResponse;
use codex_app_server_protocol::ContextMapRegionAnchor as ApiContextMapRegionAnchor;
use codex_app_server_protocol::ContextMapSource as ApiContextMapSource;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_project_intelligence::ContextMapCoverage;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapHit;
use codex_project_intelligence::ContextMapQuery;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::ContextMapStoreError;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectIndexerError;
use codex_state::SqliteConfig;
use codex_thread_store::StoredProject;
use codex_thread_store::ThreadStore;
use codex_thread_store::ThreadStoreError;
use codex_utils_absolute_path::AbsolutePathBuf;
use tokio::sync::OnceCell;

use crate::error_code::internal_error;
use crate::error_code::invalid_params;
use crate::error_code::method_not_found;

const DEFAULT_QUERY_LIMIT: u32 = 10;

#[derive(Clone)]
pub(crate) struct ContextMapRequestProcessor {
    thread_store: Arc<dyn ThreadStore>,
    sqlite: Option<SqliteConfig>,
    context_map: Arc<OnceCell<ContextMapStore>>,
    hierarchy: Arc<OnceCell<HierarchyStore>>,
}

impl ContextMapRequestProcessor {
    pub(crate) fn new(thread_store: Arc<dyn ThreadStore>, sqlite: Option<SqliteConfig>) -> Self {
        Self {
            thread_store,
            sqlite,
            context_map: Arc::new(OnceCell::new()),
            hierarchy: Arc::new(OnceCell::new()),
        }
    }

    pub(crate) async fn context_map_refresh(
        &self,
        params: ContextMapRefreshParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let sqlite = self.sqlite.as_ref().ok_or_else(|| {
            method_not_found("contextMap/refresh is unavailable without sqlite state")
        })?;
        let project = self
            .thread_store
            .read_project(params.project_id.clone())
            .await
            .map_err(context_map_project_error)?
            .ok_or_else(|| invalid_params(format!("project not found: {}", params.project_id)))?;
        let hierarchy = self
            .hierarchy
            .get_or_try_init(|| HierarchyStore::open(sqlite))
            .await
            .map_err(|error| {
                internal_error(format!("failed to open project hierarchy: {error}"))
            })?;
        let report = ProjectIndexer::new(hierarchy.clone(), self.store().await?.clone())
            .refresh(ProjectIndexRequest {
                project_id: project.id,
                roots: project
                    .roots
                    .into_iter()
                    .map(|root| PathBuf::from(root.path))
                    .collect(),
            })
            .await
            .map_err(context_map_refresh_error)?;
        Ok(Some(
            ContextMapRefreshResponse {
                files_indexed: report.files_indexed,
                regions_indexed: report.regions_indexed,
                files_skipped: report.files_skipped,
                missing_files: report.missing_files,
                truncated: report.truncated,
                scan_duration_ms: report.scan_duration_ms,
                publication_duration_ms: report.publication_duration_ms,
            }
            .into(),
        ))
    }

    pub(crate) async fn context_map_query(
        &self,
        params: ContextMapQueryParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let store = self.store().await?;
        let project = self
            .thread_store
            .read_project(params.project_id.clone())
            .await
            .map_err(context_map_project_error)?
            .ok_or_else(|| invalid_params(format!("project not found: {}", params.project_id)))?;
        let hits = store
            .query(ContextMapQuery {
                project_id: params.project_id,
                text: params.text,
                max_results: params.limit.unwrap_or(DEFAULT_QUERY_LIMIT),
            })
            .await
            .map_err(context_map_store_error)?;
        Ok(Some(
            ContextMapQueryResponse {
                data: hits
                    .into_iter()
                    .map(|hit| api_hit(hit, &project))
                    .collect::<Result<_, _>>()?,
            }
            .into(),
        ))
    }

    async fn store(&self) -> Result<&ContextMapStore, JSONRPCErrorError> {
        let sqlite = self.sqlite.as_ref().ok_or_else(|| {
            method_not_found("contextMap/query is unavailable without sqlite state")
        })?;
        self.context_map
            .get_or_try_init(|| ContextMapStore::open(sqlite))
            .await
            .map_err(context_map_store_error)
    }
}

fn api_hit(
    hit: ContextMapHit,
    project: &StoredProject,
) -> Result<ApiContextMapQueryHit, JSONRPCErrorError> {
    if !project
        .roots
        .iter()
        .any(|root| root.path == hit.source.project_root)
    {
        return Err(internal_error(format!(
            "stored context-map root is not selected for project {}",
            project.id
        )));
    }
    let project_root = AbsolutePathBuf::from_absolute_path(PathBuf::from(&hit.source.project_root))
        .map_err(|error| {
            internal_error(format!("stored context-map root is not absolute: {error}"))
        })?;
    Ok(ApiContextMapQueryHit {
        entry_id: hit.entry.id.to_string(),
        node_id: hit.entry.value.node_id.to_string(),
        source_fingerprint: hit.entry.value.source_fingerprint.to_string(),
        description: hit.entry.value.description,
        routing_terms: hit.entry.value.routing_terms,
        coverage: match hit.entry.value.coverage {
            ContextMapCoverage::Complete => ApiContextMapCoverage::Complete,
            ContextMapCoverage::Partial => ApiContextMapCoverage::Partial,
        },
        freshness: match hit.freshness {
            ContextMapFreshness::Current => ApiContextMapFreshness::Current,
            ContextMapFreshness::Stale => ApiContextMapFreshness::Stale,
            ContextMapFreshness::SourceUnavailable => ApiContextMapFreshness::SourceUnavailable,
        },
        source: ApiContextMapSource {
            project_root,
            relative_path: hit.source.relative_path.to_string(),
            region_anchor: hit
                .source
                .region_anchor
                .map(|anchor| ApiContextMapRegionAnchor {
                    scheme: anchor.scheme,
                    locator: anchor.locator,
                }),
        },
        revision: hit.entry.revision,
        created_at: hit.entry.created_at_ms.div_euclid(/*rhs*/ 1000),
        updated_at: hit.entry.updated_at_ms.div_euclid(/*rhs*/ 1000),
        last_verified_at: hit
            .entry
            .last_verified_at_ms
            .map(|timestamp| timestamp.div_euclid(/*rhs*/ 1000)),
    })
}

fn context_map_project_error(error: ThreadStoreError) -> JSONRPCErrorError {
    match error {
        ThreadStoreError::Unsupported { .. } => {
            method_not_found("contextMap/query is unavailable without sqlite state")
        }
        ThreadStoreError::InvalidRequest { message } => invalid_params(message),
        error => internal_error(format!("failed to read context-map project: {error}")),
    }
}

fn context_map_store_error(error: ContextMapStoreError) -> JSONRPCErrorError {
    match error {
        ContextMapStoreError::InvalidEntry(error) => invalid_params(error.to_string()),
        error => internal_error(format!("failed to query context map: {error}")),
    }
}

fn context_map_refresh_error(error: ProjectIndexerError) -> JSONRPCErrorError {
    match error {
        ProjectIndexerError::InvalidRequest | ProjectIndexerError::InvalidRoot => {
            invalid_params(error.to_string())
        }
        error => internal_error(format!("failed to refresh context map: {error}")),
    }
}
