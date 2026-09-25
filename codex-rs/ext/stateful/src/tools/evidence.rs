use std::path::PathBuf;
use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::EvidenceReadError;
use codex_project_intelligence::EvidenceReadRequest;
use codex_project_intelligence::EvidenceReadResult;
use codex_project_intelligence::EvidenceReader;
use codex_project_intelligence::ProjectIndexFileRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_project_intelligence::ProjectRelativePath;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::fits_response;
use super::parse_arguments;

const TOOL_NAME: &str = "evidence_read";
const DEFAULT_BYTES: u32 = 8 * 1024;
const MAX_BYTES: u32 = 12 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvidenceArguments {
    relative_path: String,
    project_root: Option<String>,
    line_range: Option<LineRangeArguments>,
    max_bytes: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LineRangeArguments {
    start: u64,
    end: u64,
}

pub(super) struct EvidenceReadTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
}

impl EvidenceReadTool {
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
        let arguments: EvidenceArguments = parse_arguments(&call)?;
        let requested_max_bytes = arguments.max_bytes.unwrap_or(DEFAULT_BYTES);
        if requested_max_bytes == 0 {
            return Err(FunctionCallError::RespondToModel(format!(
                "maxBytes must be between 1 and {MAX_BYTES}"
            )));
        }
        let max_bytes = requested_max_bytes.min(MAX_BYTES);
        let project = self
            .projects
            .read_project(self.project_id.clone())
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel("selected project no longer exists".to_string())
            })?;
        let project_roots = project
            .roots
            .iter()
            .map(|root| PathBuf::from(&root.path))
            .collect::<Vec<_>>();
        let project_root = arguments.project_root.map(PathBuf::from);
        let relative_path = ProjectRelativePath::parse(arguments.relative_path).map_err(respond)?;
        let line_range = arguments.line_range.map(|range| EvidenceLineRange {
            start: range.start,
            end: range.end,
        });
        let (result, source_refreshed) = self
            .read_with_refresh(
                project_roots,
                project_root,
                relative_path,
                line_range,
                max_bytes,
            )
            .await?;

        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let context_map_entry_id = result.hit.entry.id.to_string();
        let returned_line_range = match (result.first_line, result.last_line) {
            (Some(start), Some(end)) if !result.truncated => Some(json!({
                "start": start,
                "end": end,
            })),
            _ => None,
        };
        let blackboard_evidence = returned_line_range.as_ref().map(|line_range| {
            json!({
                "contextMapEntryId": context_map_entry_id,
                "lineRange": line_range,
            })
        });
        let mut content = result.content;
        let original_bytes = content.len();
        let mut output = json!({
            "projectId": self.project_id,
            "contextMapEntryId": context_map_entry_id,
            "sourceFingerprint": result.hit.entry.value.source_fingerprint.to_string(),
            "source": {
                "projectRoot": result.hit.source.project_root,
                "relativePath": result.hit.source.relative_path.to_string(),
            },
            "content": content,
            "bytesReturned": result.bytes_returned,
            "totalBytes": result.total_bytes,
            "totalLines": result.total_lines,
            "firstLine": result.first_line,
            "lastLine": result.last_line,
            "truncated": result.truncated,
            "maxBytesApplied": max_bytes,
            "maxBytesClamped": requested_max_bytes != max_bytes,
            "sourceRefreshed": source_refreshed,
            "blackboardEvidence": blackboard_evidence,
            "revision": result.hit.entry.revision,
        });
        if !fits_response(&output, byte_budget) {
            output["content"] = json!("");
            output["bytesReturned"] = json!(0);
            output["truncated"] = json!(true);
            output["blackboardEvidence"] = serde_json::Value::Null;
            if !fits_response(&output, byte_budget) {
                return Err(FunctionCallError::RespondToModel(
                    "response budget leaves no room for evidence metadata".to_string(),
                ));
            }
            let mut lower = 0;
            let mut upper = content.len();
            while lower < upper {
                let end = content.ceil_char_boundary(lower.midpoint(upper).saturating_add(1));
                output["content"] = json!(&content[..end]);
                output["bytesReturned"] = json!(end);
                if fits_response(&output, byte_budget) {
                    lower = end;
                } else {
                    upper = content.floor_char_boundary(end.saturating_sub(1));
                }
            }
            content.truncate(lower);
            output["content"] = json!(content);
            output["bytesReturned"] = json!(content.len());
        }
        debug_assert!(content.len() <= original_bytes);
        Ok(Box::new(JsonToolOutput::new(output)))
    }

    async fn read_with_refresh(
        &self,
        project_roots: Vec<PathBuf>,
        project_root: Option<PathBuf>,
        relative_path: ProjectRelativePath,
        line_range: Option<EvidenceLineRange>,
        max_bytes: u32,
    ) -> Result<(EvidenceReadResult, bool), FunctionCallError> {
        let reader =
            EvidenceReader::new(self.services.context_map().await.map_err(respond)?.clone());
        let request = EvidenceReadRequest {
            project_id: self.project_id.clone(),
            project_roots: project_roots.clone(),
            project_root: project_root.clone(),
            relative_path: relative_path.clone(),
            line_range,
            max_bytes,
        };
        match reader.read(request.clone()).await {
            Ok(result) => Ok((result, false)),
            Err(
                EvidenceReadError::SourceChanged
                | EvidenceReadError::SourceNotCurrent(ContextMapFreshness::Stale),
            ) => {
                let project_root = self
                    .refresh_root(&project_roots, project_root.as_ref(), &relative_path)
                    .await?;
                ProjectIndexer::new(
                    self.services.hierarchy().await.map_err(respond)?.clone(),
                    self.services.context_map().await.map_err(respond)?.clone(),
                )
                .refresh_file(ProjectIndexFileRequest {
                    project_id: self.project_id.clone(),
                    project_root,
                    relative_path,
                })
                .await
                .map_err(respond)?;
                reader
                    .read(request)
                    .await
                    .map(|result| (result, true))
                    .map_err(respond)
            }
            Err(error) => Err(respond(error)),
        }
    }

    async fn refresh_root(
        &self,
        project_roots: &[PathBuf],
        requested_root: Option<&PathBuf>,
        relative_path: &ProjectRelativePath,
    ) -> Result<PathBuf, FunctionCallError> {
        if let Some(root) = requested_root {
            return Ok(root.clone());
        }
        if let [root] = project_roots {
            return Ok(root.clone());
        }
        let hits = self
            .services
            .context_map()
            .await
            .map_err(respond)?
            .file_hits_for_path(&self.project_id, relative_path)
            .await
            .map_err(respond)?;
        let mut matching_roots = hits
            .into_iter()
            .filter_map(|hit| {
                project_roots
                    .iter()
                    .find(|root| *root == &PathBuf::from(&hit.source.project_root))
                    .cloned()
            })
            .collect::<Vec<_>>();
        matching_roots.dedup();
        match matching_roots.as_slice() {
            [root] => Ok(root.clone()),
            _ => Err(FunctionCallError::RespondToModel(format!(
                "source path exists in multiple project roots; provide projectRoot: {relative_path}"
            ))),
        }
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for EvidenceReadTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Read a fingerprint-verified exact source or line range through the selected project's context map. When root blackboard evidence already names a source and lines, prefer this focused tool over a context-map search or broad shell read. Request only the smallest line range whose wording can change the answer. A changed indexed file is refreshed once and reread; sourceRefreshed=true means prior knowledge tied to the old fingerprint remains stale and must be revised or superseded before reuse. When blackboardEvidence is non-null, copy that object unchanged into a blackboard record's evidence array so the persisted locator exactly matches the verified text. A null value means the returned text was incomplete and must not be recorded as exact line evidence.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "relativePath": {"type": "string", "description": "Project-relative path shown by the root blackboard or context map."},
                    "projectRoot": {"type": "string", "description": "Required only when the same relative path exists under multiple selected roots."},
                    "lineRange": {
                        "type": "object",
                        "description": "Optional inclusive 1-based line range. Prefer a narrow range when known.",
                        "properties": {
                            "start": {"type": "integer", "minimum": 1},
                            "end": {"type": "integer", "minimum": 1}
                        },
                        "required": ["start", "end"],
                        "additionalProperties": false
                    },
                    "maxBytes": {"type": "integer", "minimum": 1, "maximum": MAX_BYTES}
                },
                "required": ["relativePath"],
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static evidence read schema: {error}")),
            output_schema: None,
        })
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

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
