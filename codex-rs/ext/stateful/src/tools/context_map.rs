use std::collections::HashMap;
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
use codex_project_intelligence::BlackboardRouteKnowledge;
use codex_project_intelligence::BlackboardRouteKnowledgeQuery;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapHit;
use codex_project_intelligence::ContextMapListQuery;
use codex_project_intelligence::ContextMapQuery;
use codex_project_intelligence::EvidenceRoute;
use codex_project_intelligence::ProjectIndexRequest;
use codex_project_intelligence::ProjectIndexer;
use codex_thread_store::ThreadStore;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::audited_context_freshness;
use crate::source_freshness::observe_evidence;

use super::MAX_RESPONSE_BYTES;
use super::fits_response;
use super::parse_arguments;

const TOOL_NAME: &str = "context_map_query";
const REFRESH_TOOL_NAME: &str = "context_map_refresh";
const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 20;
const REFRESH_ROUTE_LIMIT: u32 = 20;
const MAX_HEADLINE_BYTES: usize = 240;

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
        let query_text = arguments.text;
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
                text: query_text.clone(),
                max_results: limit,
            })
            .await
            .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let may_have_more = hits.len() == limit as usize;
        let (knowledge, knowledge_coverage_available) =
            route_knowledge(&self.services, &self.project_id, &hits).await;
        let roots = project
            .roots
            .iter()
            .map(|root| PathBuf::from(&root.path))
            .collect::<Vec<_>>();
        let evidence_audit = observe_evidence(
            &self.services,
            &self.project_id,
            &roots,
            hits.iter().map(|hit| hit.entry.id.clone()),
        )
        .await;
        let mut data = Vec::new();
        let mut truncated = false;
        for hit in hits {
            let known = knowledge.get(&hit.entry.id);
            let freshness =
                audited_context_freshness(&evidence_audit, &hit.entry.id, hit.freshness);
            let item = route_json(hit, &project, known, freshness, Some(&query_text))?;
            data.push(item);
            if !fits_response(
                &json!({
                    "projectId": self.project_id,
                    "data": &data,
                    "truncated": truncated,
                    "mayHaveMore": may_have_more,
                    "knowledgeCoverageAvailable": knowledge_coverage_available,
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
            "knowledgeCoverageAvailable": knowledge_coverage_available,
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
            description: "Locate exact project files or anchored regions when project intelligence lacks required detail or a controlling scope, authority, or supersession boundary; reports stale/unchecked evidence or a conflict; exact source wording or format is needed; or the user requests fresh verification. Returned routes are byte-checked without mutating project state: freshness is the live observation and storedFreshness is the persisted index state. Headlines are bounded match-centered routing previews, not evidence. Pass a current evidenceRoute unchanged to evidence_read; it is bound to the returned source revision and exact range. Reuse adequate root knowledge without a confirming read. When a route reports knownKnowledge, treat it as coverage only; use already-loaded root knowledge or query deeper blackboard knowledge before reading raw evidence.".to_string(),
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
                .iter()
                .map(|root| PathBuf::from(&root.path))
                .collect(),
        })
        .await
        .map_err(respond)?;
        let routes = self
            .services
            .context_map()
            .await
            .map_err(respond)?
            .list_project(ContextMapListQuery {
                project_id: self.project_id.clone(),
                max_results: REFRESH_ROUTE_LIMIT,
            })
            .await
            .map_err(respond)?;
        let byte_budget = call.response_byte_budget(MAX_RESPONSE_BYTES);
        let (knowledge, knowledge_coverage_available) =
            route_knowledge(&self.services, &self.project_id, &routes).await;
        let mut data = Vec::new();
        let mut routes_truncated = routes.len() == REFRESH_ROUTE_LIMIT as usize;
        for hit in routes {
            let known = knowledge.get(&hit.entry.id);
            let freshness = Some(hit.freshness);
            let item = route_json(hit, &project, known, freshness, /*query_text*/ None)?;
            data.push(item);
            if !fits_response(
                &json!({
                    "projectId": self.project_id,
                    "filesIndexed": report.files_indexed,
                    "filesSkipped": report.files_skipped,
                    "missingFiles": report.missing_files,
                    "truncated": report.truncated,
                    "routes": &data,
                    "routesTruncated": routes_truncated,
                    "knowledgeCoverageAvailable": knowledge_coverage_available,
                }),
                byte_budget,
            ) {
                data.pop();
                routes_truncated = true;
                break;
            }
        }
        Ok(Box::new(JsonToolOutput::new(json!({
            "projectId": self.project_id,
            "filesIndexed": report.files_indexed,
            "filesSkipped": report.files_skipped,
            "missingFiles": report.missing_files,
            "truncated": report.truncated,
            "routes": data,
            "routesTruncated": routes_truncated,
            "knowledgeCoverageAvailable": knowledge_coverage_available,
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
            description: "Refresh the selected project's filesystem hierarchy and source-routing index. Use when the context map is empty or project files changed. The result includes a bounded source-route inventory; use those routes directly and query the context map only when the inventory is truncated or does not identify the needed source.".to_string(),
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

fn route_json(
    hit: ContextMapHit,
    project: &codex_thread_store::StoredProject,
    knowledge: Option<&BlackboardRouteKnowledge>,
    freshness: Option<ContextMapFreshness>,
    query_text: Option<&str>,
) -> Result<serde_json::Value, FunctionCallError> {
    if !project
        .roots
        .iter()
        .any(|root| root.path == hit.source.project_root)
    {
        return Err(FunctionCallError::RespondToModel(
            "stored context-map route is outside the selected project roots".to_string(),
        ));
    }
    let evidence_route = (freshness == Some(ContextMapFreshness::Current))
        .then(|| EvidenceRoute::from_hit(&hit))
        .transpose()
        .map_err(respond)?;
    let headline = bounded_headline(&hit.entry.value.description, query_text);
    let mut source = serde_json::Map::from_iter([(
        "relativePath".to_string(),
        json!(hit.source.relative_path.to_string()),
    )]);
    if project.roots.len() > 1 {
        source.insert("projectRoot".to_string(), json!(hit.source.project_root));
    }
    if let Some(region_anchor) = hit.source.region_anchor {
        source.insert("regionAnchor".to_string(), json!(region_anchor));
    }
    let mut route = json!({
        "headline": headline,
        "coverage": hit.entry.value.coverage,
        "freshness": freshness.map(freshness_name).unwrap_or("uncheckedThisTurn"),
        "storedFreshness": freshness_name(hit.freshness),
        "source": source,
    });
    if let Some(evidence_route) = evidence_route {
        route["evidenceRoute"] = json!(evidence_route);
    }
    if let Some(knowledge) = knowledge.filter(|knowledge| knowledge.active_entries > 0) {
        route["knownKnowledge"] = json!({
            "rootEntries": knowledge.root_entries,
            "deeperEntries": knowledge.active_entries.saturating_sub(knowledge.root_entries),
        });
    }
    Ok(route)
}

fn bounded_headline(description: &str, query_text: Option<&str>) -> String {
    if description.len() <= MAX_HEADLINE_BYTES {
        return description.to_string();
    }
    let marker = "…";
    let Some(query_text) = query_text else {
        let mut end = MAX_HEADLINE_BYTES.saturating_sub(marker.len());
        while !description.is_char_boundary(end) {
            end -= 1;
        }
        return format!("{}{marker}", &description[..end]);
    };
    let description_lower = description.to_ascii_lowercase();
    let body_start = description
        .find(" | ")
        .map_or(/*default*/ 0, |position| position + 3);
    let mut terms = Vec::new();
    for term in query_text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|term| term.len() > 1)
        .take(/*n*/ 32)
        .map(str::to_ascii_lowercase)
    {
        if !terms.contains(&term) {
            terms.push(term);
        }
    }
    let content_budget = MAX_HEADLINE_BYTES.saturating_sub(marker.len() * 2);
    let mut best = None;
    for term in &terms {
        for (position, _) in description_lower.match_indices(term).take(/*n*/ 4) {
            if position < body_start {
                continue;
            }
            let mut start = position.saturating_sub(content_budget / 3);
            while !description.is_char_boundary(start) {
                start -= 1;
            }
            let mut end = (start + content_budget).min(description.len());
            while !description.is_char_boundary(end) {
                end -= 1;
            }
            let window = &description_lower[start..end];
            let coverage = terms
                .iter()
                .filter(|term| window.contains(term.as_str()))
                .count();
            let candidate = (coverage, term.len(), std::cmp::Reverse(position), start);
            if best.as_ref().is_none_or(|current| candidate > *current) {
                best = Some(candidate);
            }
        }
    }
    let Some((_, _, _, start)) = best else {
        let mut end = MAX_HEADLINE_BYTES.saturating_sub(marker.len());
        while !description.is_char_boundary(end) {
            end -= 1;
        }
        return format!("{}{marker}", &description[..end]);
    };
    let mut end = (start + content_budget).min(description.len());
    while !description.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}{}{}",
        if start > 0 { marker } else { "" },
        &description[start..end],
        if end < description.len() { marker } else { "" },
    )
}

async fn route_knowledge(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    hits: &[ContextMapHit],
) -> (HashMap<ContextMapEntryId, BlackboardRouteKnowledge>, bool) {
    if hits.is_empty() {
        return (HashMap::new(), true);
    }
    let store = match services.blackboard().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open route knowledge store");
            return (HashMap::new(), false);
        }
    };
    match store
        .route_knowledge(BlackboardRouteKnowledgeQuery {
            project_id: project_id.to_string(),
            context_map_entry_ids: hits.iter().map(|hit| hit.entry.id.clone()).collect(),
        })
        .await
    {
        Ok(knowledge) => (
            knowledge
                .into_iter()
                .map(|knowledge| (knowledge.context_map_entry_id.clone(), knowledge))
                .collect(),
            true,
        ),
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to load route knowledge coverage");
            (HashMap::new(), false)
        }
    }
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}
