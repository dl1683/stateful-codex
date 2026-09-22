use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExposure;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::HierarchyNodeId;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;

use super::MAX_RESPONSE_BYTES;
use super::fits_response;
use super::parse_arguments;

const TOOL_NAME: &str = "blackboard_query";
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 50;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryArguments {
    text: Option<String>,
    within_node_id: Option<String>,
    limit: Option<u32>,
}

pub(super) struct BlackboardQueryTool {
    project_id: String,
    services: ProjectIntelligenceServices,
}

impl BlackboardQueryTool {
    pub(super) fn new(project_id: String, services: ProjectIntelligenceServices) -> Self {
        Self {
            project_id,
            services,
        }
    }

    async fn handle_call(
        &self,
        call: ToolCall<'_>,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, FunctionCallError> {
        let arguments: QueryArguments = parse_arguments(&call)?;
        let within_node = arguments
            .within_node_id
            .map(HierarchyNodeId::parse)
            .transpose()
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let limit = arguments.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let result = self
            .services
            .blackboard()
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?
            .query(BlackboardQuery {
                project_id: self.project_id.clone(),
                text: arguments.text,
                within_node,
                max_results: limit,
            })
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let mut data = Vec::new();
        let mut truncated = result.truncated;
        for hit in result.data {
            let item = json!({
                "entryId": hit.entry.id.to_string(),
                "nodeId": hit.entry.value.node_id.to_string(),
                "revision": hit.entry.revision,
                "kind": hit.entry.value.kind,
                "content": hit.entry.value.content,
                "structuredValue": hit.entry.value.structured_value,
                "confidenceBasisPoints": hit.entry.value.confidence.basis_points(),
                "declaredVerification": hit.entry.value.verification,
                "effectiveVerification": hit.effective_verification,
                "evidenceFreshness": hit.evidence_freshness,
                "importance": hit.entry.value.importance,
                "evidence": hit.entry.value.evidence.into_iter().map(|link| json!({
                    "contextMapEntryId": link.context_map_entry_id.to_string(),
                    "sourceFingerprint": link.source_fingerprint.to_string(),
                })).collect::<Vec<_>>(),
                "provenance": hit.entry.value.provenance,
                "relations": hit.relations.into_iter().map(|relation| json!({
                    "relationId": relation.id.to_string(),
                    "revision": relation.revision,
                    "fromEntryId": relation.value.from_entry_id.to_string(),
                    "toEntryId": relation.value.to_entry_id.to_string(),
                    "kind": relation.value.kind,
                    "note": relation.value.note,
                    "confidenceBasisPoints": relation.value.confidence.basis_points(),
                    "provenance": relation.value.provenance,
                })).collect::<Vec<_>>(),
            });
            data.push(item);
            if !fits_response(
                &json!({
                    "projectId": self.project_id,
                    "data": &data,
                    "truncated": truncated,
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
        }))))
    }
}

impl<'call> ToolExecutor<ToolCall<'call>> for BlackboardQueryTool {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Query accumulated project understanding when the root blackboard lacks needed detail. Prefer a focused text query and the smallest useful limit; omit text only when intentionally enumerating a bounded subtree.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Optional literal topic search. Prefer this when looking for specific knowledge."},
                    "withinNodeId": {"type": "string", "description": "Optional hierarchy node whose subtree bounds the query."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "Maximum records to return. Use the smallest useful value."}
                },
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static blackboard schema: {error}")),
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
