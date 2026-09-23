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
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::EvidenceReadRequest;
use codex_project_intelligence::EvidenceReader;
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
        let result =
            EvidenceReader::new(self.services.context_map().await.map_err(respond)?.clone())
                .read(EvidenceReadRequest {
                    project_id: self.project_id.clone(),
                    project_roots: project
                        .roots
                        .into_iter()
                        .map(|root| PathBuf::from(root.path))
                        .collect(),
                    project_root: arguments.project_root.map(PathBuf::from),
                    relative_path: ProjectRelativePath::parse(arguments.relative_path)
                        .map_err(respond)?,
                    line_range: arguments.line_range.map(|range| EvidenceLineRange {
                        start: range.start,
                        end: range.end,
                    }),
                    max_bytes,
                })
                .await
                .map_err(respond)?;

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
}

impl<'call> ToolExecutor<ToolCall<'call>> for EvidenceReadTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Read a fingerprint-verified exact source or line range through the selected project's context map. When root blackboard evidence already names a source and lines, prefer this focused tool over a context-map search or broad shell read. Request only the smallest line range whose wording can change the answer; changed or stale sources are rejected. When blackboardEvidence is non-null, copy that object unchanged into a blackboard record's evidence array so the persisted locator exactly matches the verified text. A null value means the returned text was incomplete and must not be recorded as exact line evidence.".to_string(),
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
