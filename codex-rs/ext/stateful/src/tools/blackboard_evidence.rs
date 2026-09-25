use std::collections::HashSet;
use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEvidenceLink;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::HierarchyNodeId;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::audited_context_freshness;
use crate::source_freshness::observe_evidence;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EvidenceArguments {
    read_receipt_id: Option<String>,
    context_map_entry_id: Option<String>,
    relative_path: Option<String>,
    project_root: Option<String>,
    line_range: Option<codex_project_intelligence::EvidenceLineRange>,
}

pub(super) async fn resolve_evidence(
    project_id: &str,
    thread_id: &str,
    services: &ProjectIntelligenceServices,
    project_roots: &[PathBuf],
    arguments: Vec<EvidenceArguments>,
) -> Result<(Vec<BlackboardEvidenceLink>, Option<HierarchyNodeId>), FunctionCallError> {
    let store = services.context_map().await.map_err(respond)?;
    let mut links = Vec::with_capacity(arguments.len());
    let mut seen_locators = HashSet::with_capacity(arguments.len());
    let mut node_ids = HashSet::with_capacity(arguments.len());
    for argument in arguments {
        let EvidenceArguments {
            read_receipt_id,
            context_map_entry_id,
            relative_path,
            project_root,
            line_range,
        } = argument;
        let legacy_locator_present = context_map_entry_id.is_some()
            || relative_path.is_some()
            || project_root.is_some()
            || line_range.is_some();
        if legacy_locator_present {
            return Err(FunctionCallError::RespondToModel(
                "blackboard evidence must use the host-issued readReceiptId returned by evidence_read; route locators alone do not prove which source version the model read"
                    .to_string(),
            ));
        }
        let receipt_id = read_receipt_id.ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "blackboard evidence requires readReceiptId from evidence_read".to_string(),
            )
        })?;
        let receipt = services
            .read_receipts()
            .resolve(project_id, thread_id, &receipt_id)
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "blackboard evidence read receipt is unknown, expired, or belongs to another thread; call evidence_read again"
                        .to_string(),
                )
            })?;
        let link = receipt.evidence;
        let hit = store
            .get_hit(project_id, &link.context_map_entry_id)
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "context-map evidence entry not found: {}",
                    link.context_map_entry_id
                ))
            })?;
        if hit.freshness != ContextMapFreshness::Current {
            return Err(FunctionCallError::RespondToModel(format!(
                "context-map evidence is not current: {}",
                hit.entry.id
            )));
        }
        if hit.entry.value.source_fingerprint != link.source_fingerprint {
            return Err(FunctionCallError::RespondToModel(format!(
                "blackboard evidence source changed after it was read: {}; call evidence_read again before recording knowledge",
                hit.entry.id
            )));
        }
        node_ids.insert(hit.entry.value.node_id.clone());
        if seen_locators.insert((link.context_map_entry_id.clone(), link.line_range)) {
            links.push(link);
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
        "description": "Host-issued receipts linked to sourceVerified knowledge. Copy each non-null blackboardEvidence object returned by evidence_read unchanged. The host resolves the exact route, source fingerprint, and complete returned line range and rejects receipts from another thread or a source that changed after reading. Semantic entailment remains the model's responsibility.",
        "items": {
            "type": "object",
            "properties": {
                "readReceiptId": {"type": "string", "description": "Opaque receipt returned by evidence_read for the exact source bytes reviewed."}
            },
            "required": ["readReceiptId"],
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
