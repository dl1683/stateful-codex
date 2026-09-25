use std::collections::HashSet;
use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::EvidenceLineRange;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::ProjectRelativePath;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::audited_context_freshness;
use crate::source_freshness::observe_evidence;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EvidenceArguments {
    context_map_entry_id: Option<String>,
    relative_path: Option<String>,
    project_root: Option<String>,
    line_range: Option<EvidenceLineRange>,
}

pub(super) async fn resolve_evidence(
    project_id: &str,
    services: &ProjectIntelligenceServices,
    project_roots: &[PathBuf],
    arguments: Vec<EvidenceArguments>,
) -> Result<(Vec<BlackboardEvidenceLink>, Option<HierarchyNodeId>), FunctionCallError> {
    let store = services.context_map().await.map_err(respond)?;
    let mut links = Vec::with_capacity(arguments.len());
    let mut seen_locators = HashSet::with_capacity(arguments.len());
    let mut node_ids = HashSet::with_capacity(arguments.len());
    for argument in arguments {
        let line_range = argument.line_range;
        let hit = match (
            argument.context_map_entry_id,
            argument.relative_path,
            argument.project_root,
        ) {
            (Some(raw_id), None, None) => {
                let id = ContextMapEntryId::parse(raw_id).map_err(respond)?;
                store
                    .get_hit(project_id, &id)
                    .await
                    .map_err(respond)?
                    .ok_or_else(|| {
                        FunctionCallError::RespondToModel(format!(
                            "context-map evidence entry not found: {id}"
                        ))
                    })?
            }
            (None, Some(raw_path), project_root) => {
                let relative_path = ProjectRelativePath::parse(raw_path).map_err(respond)?;
                let mut hits = store
                    .file_hits_for_path(project_id, &relative_path)
                    .await
                    .map_err(respond)?
                    .into_iter()
                    .filter(|hit| {
                        hit.freshness == ContextMapFreshness::Current
                            && project_root
                                .as_ref()
                                .is_none_or(|root| root == &hit.source.project_root)
                    });
                let hit = hits.next().ok_or_else(|| {
                    FunctionCallError::RespondToModel(format!(
                        "no current context-map evidence route found for {relative_path}"
                    ))
                })?;
                if hits.next().is_some() {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "multiple current context-map routes match {relative_path}; provide projectRoot or contextMapEntryId"
                    )));
                }
                hit
            }
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "each evidence item must provide exactly one of contextMapEntryId or relativePath; projectRoot is valid only with relativePath"
                        .to_string(),
                ));
            }
        };
        if hit.freshness != ContextMapFreshness::Current {
            return Err(FunctionCallError::RespondToModel(format!(
                "context-map evidence is not current: {}",
                hit.entry.id
            )));
        }
        node_ids.insert(hit.entry.value.node_id.clone());
        if seen_locators.insert((hit.entry.id.clone(), line_range)) {
            links.push(BlackboardEvidenceLink {
                context_map_entry_id: hit.entry.id,
                source_fingerprint: hit.entry.value.source_fingerprint,
                line_range,
            });
        }
    }
    if !links.is_empty() {
        let audit = observe_evidence(
            services,
            project_id,
            project_roots,
            links.iter().map(|link| link.context_map_entry_id.clone()),
        )
        .await;
        for link in &links {
            match audited_context_freshness(
                &audit,
                &link.context_map_entry_id,
                ContextMapFreshness::Current,
            ) {
                Some(ContextMapFreshness::Current) => {}
                Some(ContextMapFreshness::Stale) => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "blackboard evidence source changed: {}; call evidence_read to refresh and reread it before recording knowledge",
                        link.context_map_entry_id
                    )));
                }
                Some(ContextMapFreshness::SourceUnavailable) => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "blackboard evidence source is unavailable: {}",
                        link.context_map_entry_id
                    )));
                }
                None => {
                    return Err(FunctionCallError::RespondToModel(format!(
                        "blackboard evidence could not be checked within the live freshness bound: {}; reread a smaller current source before recording source-verified knowledge",
                        link.context_map_entry_id
                    )));
                }
            }
        }
    }
    let inferred_node_id = if node_ids.len() == 1 {
        node_ids.into_iter().next()
    } else {
        None
    };
    Ok((links, inferred_node_id))
}

pub(super) fn evidence_schema() -> serde_json::Value {
    json!({
        "type": "array",
        "description": "Current context-map routes linked to sourceVerified knowledge. Prefer copying the blackboardEvidence object returned by evidence_read unchanged; it contains the exact route and complete returned line range. Otherwise each item must use exactly one route identity: relativePath (plus projectRoot only when paths collide) or contextMapEntryId for an anchored/exact route. Never send both. IDs and fingerprints are resolved and checked by the tool; semantic entailment is still the model's responsibility.",
        "items": {
            "type": "object",
            "properties": {
                "contextMapEntryId": {"type": "string", "description": "Exact route identity. Exclusive with relativePath and projectRoot."},
                "relativePath": {"type": "string", "description": "Preferred project-relative route. Exclusive with contextMapEntryId."},
                "projectRoot": {"type": "string", "description": "Optional only with relativePath when multiple selected roots contain the same path."},
                "lineRange": {
                    "type": "object",
                    "description": "Exact 1-based inclusive source lines copied from a non-null evidence_read blackboardEvidence result. Omit unless that exact complete range was read.",
                    "properties": {
                        "start": {"type": "integer", "minimum": 1},
                        "end": {"type": "integer", "minimum": 1}
                    },
                    "required": ["start", "end"],
                    "additionalProperties": false
                }
            },
            "oneOf": [
                {"required": ["contextMapEntryId"]},
                {"required": ["relativePath"]}
            ],
            "additionalProperties": false
        }
    })
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "blackboard_evidence_tests.rs"]
mod tests;
