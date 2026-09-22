use std::path::PathBuf;
use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapQuery;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::fits_response;
use super::parse_arguments;

const TOOL_NAME: &str = "context_map_query";
const REFRESH_TOOL_NAME: &str = "context_map_refresh";
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 20;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryArguments {
    text: String,
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshArguments {}

pub(super) struct ContextMapQueryTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
}

impl ContextMapQueryTool {
    pub(super) fn new(
        project_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
    ) -> Self {
        Self {
            project_id,
            services,
            projects,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: QueryArguments = parse_arguments(&call)?;
        let limit = arguments.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let project = self
            .projects
            .read_project(self.project_id.clone())
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel("selected project no longer exists".to_string())
            })?;
        let hits = self
            .services
            .context_map()
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?
            .query(ContextMapQuery {
                project_id: self.project_id.clone(),
                text: arguments.text,
                max_results: limit,
            })
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let may_have_more = hits.len() == limit as usize;
        let mut data = Vec::new();
        let mut truncated = false;
        for hit in hits {
            if !project
                .roots
                .iter()
                .any(|root| root.path == hit.source.project_root)
            {
                return Err(FunctionCallError::RespondToModel(
                    "stored context-map route is outside the selected project roots".to_string(),
                ));
            }
            let item = json!({
                "entryId": hit.entry.id.to_string(),
                "nodeId": hit.entry.value.node_id.to_string(),
                "sourceFingerprint": hit.entry.value.source_fingerprint.to_string(),
                "description": hit.entry.value.description,
                "routingTerms": hit.entry.value.routing_terms,
                "coverage": hit.entry.value.coverage,
                "freshness": freshness_name(hit.freshness),
                "source": {
                    "projectRoot": hit.source.project_root,
                    "relativePath": hit.source.relative_path.to_string(),
                    "regionAnchor": hit.source.region_anchor,
                },
                "revision": hit.entry.revision,
            });
            data.push(item);
            if !fits_response(
                &json!({
                    "projectId": self.project_id,
                    "data": &data,
                    "truncated": truncated,
                    "mayHaveMore": may_have_more,
                }),
                byte_budget,
            ) {
                data.pop();
                truncated = true;
                break;
            }
        }
        Ok(Box::new(JsonToolOutput::new(json!({
            "projectId": self.project_id,
            "data": data,
            "truncated": truncated,
            "mayHaveMore": may_have_more,
        }))))
    }
}

fn freshness_name(value: ContextMapFreshness) -> &'static str {
    match value {
        ContextMapFreshness::Current => "current",
        ContextMapFreshness::Stale => "stale",
        ContextMapFreshness::SourceUnavailable => "sourceUnavailable",
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for ContextMapQueryTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Locate exact project files or anchored regions relevant to a question. Use returned routes to verify consequential claims against source.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Required literal search for likely source material."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT}
                },
                "required": ["text"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static context-map schema: {error}")),
            output_schema: None,
        })
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

pub(super) struct ContextMapRefreshTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
}

impl ContextMapRefreshTool {
    pub(super) fn new(
        project_id: String,
        services: ProjectIntelligenceServices,
        projects: Arc<dyn ThreadStore>,
    ) -> Self {
        Self {
            project_id,
            services,
            projects,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let _arguments: RefreshArguments = parse_arguments(&call)?;
        let project = self
            .projects
            .read_project(self.project_id.clone())
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel("selected project no longer exists".to_string())
            })?;
        let report = ProjectIndexer::new(
            self.services.hierarchy().await.map_err(respond)?.clone(),
            self.services.context_map().await.map_err(respond)?.clone(),
        )
        .refresh(ProjectIndexRequest {
            project_id: self.project_id.clone(),
            roots: project
                .roots
                .into_iter()
                .map(|root| PathBuf::from(root.path))
                .collect(),
        })
        .await
        .map_err(respond)?;
        Ok(Box::new(JsonToolOutput::new(json!({
            "projectId": self.project_id,
            "filesIndexed": report.files_indexed,
            "filesSkipped": report.files_skipped,
            "missingFiles": report.missing_files,
            "truncated": report.truncated,
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for ContextMapRefreshTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(REFRESH_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: REFRESH_TOOL_NAME.to_string(),
            description: "Refresh the selected project's filesystem hierarchy and source-routing index. Use when the context map is empty or project files changed.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static context-map refresh schema: {error}")),
            output_schema: None,
        })
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Deferred
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn handle<'a>(&'a self, call: ToolCall<'call>) -> codex_extension_api::ToolExecutorFuture<'a>
    where
        'call: 'a,
    {
        Box::pin(self.handle_call(call))
    }
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}
