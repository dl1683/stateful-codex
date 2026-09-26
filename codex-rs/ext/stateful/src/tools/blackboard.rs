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
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryScope;
use codex_project_intelligence::BlackboardEvidenceDependentsQuery;
use codex_project_intelligence::BlackboardQuery;
use codex_project_intelligence::BlackboardQueryResult;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::RootPromotion;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::audited_blackboard_freshness;
use crate::source_freshness::audited_verification;
use crate::source_freshness::observe_evidence;

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
    root_promotion: Option<RootPromotion>,
    entry_scope: Option<BlackboardEntryScope>,
    evidence_context_map_entry_ids: Option<Vec<String>>,
    expected_project_revision: Option<u64>,
    after_entry_id: Option<String>,
    limit: Option<u32>,
}

pub(super) struct BlackboardQueryTool {
    project_id: String,
    services: ProjectIntelligenceServices,
    projects: Arc<dyn ThreadStore>,
}

impl BlackboardQueryTool {
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
        let QueryArguments {
            text,
            within_node_id,
            root_promotion,
            entry_scope,
            evidence_context_map_entry_ids,
            expected_project_revision,
            after_entry_id,
            limit,
        } = parse_arguments(&call)?;
        let within_node = within_node_id
            .map(HierarchyNodeId::parse)
            .transpose()
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let entry_scope = entry_scope.unwrap_or(BlackboardEntryScope::Active);
        let after_entry_id = after_entry_id
            .map(BlackboardEntryId::parse)
            .transpose()
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let blackboard = self
            .services
            .blackboard()
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let evidence_query = evidence_context_map_entry_ids.is_some();
        let (project_revision, result) = if let Some(entry_ids) = evidence_context_map_entry_ids {
            if text.is_some() || within_node.is_some() || root_promotion.is_some() {
                return Err(FunctionCallError::RespondToModel(
                    "evidenceContextMapEntryIds can be combined only with entryScope, expectedProjectRevision, afterEntryId, and limit".to_string(),
                ));
            }
            let context_map_entry_ids = entry_ids
                .into_iter()
                .map(ContextMapEntryId::parse)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let result = blackboard
                .evidence_dependents(BlackboardEvidenceDependentsQuery {
                    project_id: self.project_id.clone(),
                    context_map_entry_ids,
                    entry_scope,
                    expected_project_revision,
                    after_entry_id,
                    max_results: limit,
                })
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            (
                Some(result.project_revision),
                BlackboardQueryResult {
                    data: result.data,
                    truncated: result.truncated,
                },
            )
        } else {
            if after_entry_id.is_some() || expected_project_revision.is_some() {
                return Err(FunctionCallError::RespondToModel(
                    "expectedProjectRevision and afterEntryId require evidenceContextMapEntryIds"
                        .to_string(),
                ));
            }
            (
                None,
                blackboard
                    .query(BlackboardQuery {
                        project_id: self.project_id.clone(),
                        text,
                        within_node,
                        root_promotion,
                        entry_scope,
                        max_results: limit,
                    })
                    .await
                    .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?,
            )
        };
        let evidence_ids = result
            .data
            .iter()
            .flat_map(|hit| &hit.entry.value.evidence)
            .map(|evidence| evidence.context_map_entry_id.clone())
            .collect::<Vec<_>>();
        let evidence_audit = if evidence_ids.is_empty() {
            None
        } else {
            let roots = match self.projects.read_project(self.project_id.clone()).await {
                Ok(Some(project)) => project
                    .roots
                    .iter()
                    .map(|root| PathBuf::from(&root.path))
                    .collect::<Vec<_>>(),
                Ok(None) => Vec::new(),
                Err(error) => {
                    tracing::warn!(
                        project_id = %self.project_id,
                        %error,
                        "failed to resolve project roots for blackboard source observation"
                    );
                    Vec::new()
                }
            };
            Some(observe_evidence(&self.services, &self.project_id, &roots, evidence_ids).await)
        };
        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let hit_count = result.data.len();
        let mut data = Vec::new();
        let mut truncated = result.truncated;
        for (index, hit) in result.data.into_iter().enumerate() {
            let evidence_freshness = audited_blackboard_freshness(&hit, evidence_audit.as_ref());
            let effective_verification =
                audited_verification(hit.entry.value.verification, evidence_freshness);
            let evidence_count = hit.entry.value.evidence.len();
            let mut item = json!({
                "entryId": hit.entry.id.to_string(),
                "nodeId": hit.entry.value.node_id.to_string(),
                "revision": hit.entry.revision,
                "state": hit.entry.state,
                "supersededBy": hit.entry.superseded_by.map(|id| id.to_string()),
                "kind": hit.entry.value.kind,
                "content": hit.entry.value.content,
                "structuredValue": hit.entry.value.structured_value,
                "confidenceBasisPoints": hit.entry.value.confidence.basis_points(),
                "declaredVerification": hit.entry.value.verification,
                "effectiveVerification": effective_verification,
                "evidenceFreshness": evidence_freshness,
                "storedEvidenceFreshness": hit.evidence_freshness,
                "importance": hit.entry.value.importance,
                "rootPromotion": hit.entry.value.root_promotion,
                "evidence": hit.entry.value.evidence.into_iter().map(|link| json!({
                    "contextMapEntryId": link.context_map_entry_id.to_string(),
                    "sourceFingerprint": link.source_fingerprint.to_string(),
                    "lineRange": link.line_range,
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
            if evidence_query {
                let serde_json::Value::Object(item) = &mut item else {
                    unreachable!("static blackboard query item should be an object");
                };
                item.remove("evidence");
                item.remove("relations");
                item.insert("evidenceCount".to_string(), json!(evidence_count));
                item.insert(
                    "detailsOmitted".to_string(),
                    json!(["evidenceLocators", "relations"]),
                );
            }
            data.push(item);
            let candidate_truncated = result.truncated || index + 1 < hit_count;
            let candidate_next_after_entry_id = if evidence_query && candidate_truncated {
                data.last()
                    .and_then(|item| item["entryId"].as_str())
                    .map(str::to_string)
            } else {
                None
            };
            if !fits_response(
                &json!({
                    "projectId": self.project_id,
                    "data": &data,
                    "truncated": candidate_truncated,
                    "projectRevision": project_revision,
                    "nextAfterEntryId": candidate_next_after_entry_id,
                }),
                byte_budget,
            ) {
                data.pop();
                truncated = true;
                break;
            }
            truncated = candidate_truncated;
        }
        let next_after_entry_id = if evidence_query && truncated {
            Some(
                data.last()
                    .and_then(|item| item["entryId"].as_str())
                    .ok_or_else(|| {
                        FunctionCallError::RespondToModel(
                            "response budget cannot fit one affected knowledge entry".to_string(),
                        )
                    })?
                    .to_string(),
            )
        } else {
            None
        };
        Ok(Box::new(JsonToolOutput::new(json!({
            "projectId": self.project_id,
            "data": data,
            "truncated": truncated,
            "projectRevision": project_revision,
            "nextAfterEntryId": next_after_entry_id,
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
            description: "Query accumulated project understanding when the root blackboard lacks needed detail or when the root reports pending candidates. Source-linked results are byte-checked against the selected project without mutating project state; evidenceFreshness is the effective observation and storedEvidenceFreshness is the persisted index state. Reuse sourceVerified knowledge only when evidenceFreshness=current. Active knowledge is the default. After evidence_read reports sourceRefreshed=true, pass its contextMapEntryId in evidenceContextMapEntryIds to enumerate entries whose current revisions directly cite the changed source through either its file route or any current or retired region route. This does not discover semantic dependencies that were never recorded. Affected-source pages omit evidence locators and navigational relations to stay resumable within the response budget. Inspect every page before mutating project intelligence, then deliberately revise or supersede stale direct dependents; retaining meaning requires fresh supporting receipts, while leaving a stale entry unchanged is not repair. The host reports mechanical dependency and freshness only and never infers semantic invalidation. The first page returns projectRevision. If truncated=true, repeat the same query with that revision as expectedProjectRevision and nextAfterEntryId copied into afterEntryId; if the project revision changes, restart from the first page. Use entryScope=historical only when reconstructing prior conclusions, failures, or superseded evidence; lifecycle state and successor identity are returned explicitly. Prefer a focused text query and the smallest useful limit; use rootPromotion=candidate to review pending root-promotion decisions, and omit text only when intentionally enumerating a bounded set.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: parse_tool_input_schema(&json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Optional literal topic search. Prefer this when looking for specific knowledge."},
                    "withinNodeId": {"type": "string", "description": "Optional hierarchy node whose subtree bounds the query."},
                    "rootPromotion": {"type": "string", "enum": ["notPromoted", "candidate", "promoted"], "description": "Optional lifecycle filter. Use candidate to review pending root-promotion decisions."},
                    "entryScope": {"type": "string", "enum": ["active", "historical", "all"], "description": "Entry lifecycle scope. Defaults to active; historical returns superseded and tombstoned entries."},
                    "evidenceContextMapEntryIds": {"type": "array", "minItems": 1, "maxItems": 20, "items": {"type": "string"}, "description": "Exact contextMapEntryId values returned by evidence_read. Each ID expands to the source file and all its file or region routes. Combine only with entryScope, expectedProjectRevision, afterEntryId, and limit."},
                    "expectedProjectRevision": {"type": "integer", "minimum": 0, "description": "For continuation pages, copy projectRevision from the first affected-source page. Omit on the first page; a mismatch fails closed and requires restarting enumeration."},
                    "afterEntryId": {"type": "string", "description": "Continuation returned as nextAfterEntryId by an affected-source query. Requires evidenceContextMapEntryIds; copy it unchanged."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "Maximum records to return. Use the smallest useful value."}
                },
                "additionalProperties": false
            }))
            .unwrap_or_else(|error| unreachable!("invalid static blackboard schema: {error}")),
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
