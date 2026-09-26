use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardPremiseLink;
use codex_project_intelligence::BlackboardVerification;
use serde::Deserialize;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::audited_blackboard_freshness;
use crate::source_freshness::audited_premise_freshness;
use crate::source_freshness::audited_verification;
use crate::source_freshness::observe_evidence;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PremiseArguments {
    entry_id: String,
    revision: u64,
}

pub(super) async fn resolve_premises(
    project_id: &str,
    services: &ProjectIntelligenceServices,
    project_roots: &[PathBuf],
    arguments: Vec<PremiseArguments>,
) -> Result<Vec<BlackboardPremiseLink>, FunctionCallError> {
    let store = services.blackboard().await.map_err(respond)?;
    let mut resolved = Vec::with_capacity(arguments.len());
    let mut hits = Vec::with_capacity(arguments.len());
    for argument in arguments {
        let entry_id = BlackboardEntryId::parse(argument.entry_id).map_err(respond)?;
        let hit = store
            .get_hit(project_id, &entry_id)
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(format!(
                    "blackboard premise entry not found: {entry_id}"
                ))
            })?;
        if hit.entry.revision != argument.revision {
            return Err(FunctionCallError::RespondToModel(format!(
                "blackboard premise revision conflict for {entry_id}: expected {}, found {}; query it again before recording the derived conclusion",
                argument.revision, hit.entry.revision
            )));
        }
        if hit.entry.state != BlackboardEntryState::Active {
            return Err(FunctionCallError::RespondToModel(format!(
                "blackboard premise is no longer active: {entry_id}"
            )));
        }
        resolved.push(BlackboardPremiseLink {
            entry_id,
            revision: argument.revision,
        });
        hits.push(hit);
    }
    let evidence_ids = hits
        .iter()
        .flat_map(|hit| {
            hit.entry
                .value
                .evidence
                .iter()
                .chain(hit.premise_evidence())
        })
        .map(|evidence| evidence.context_map_entry_id.clone())
        .collect::<Vec<_>>();
    let audit = if evidence_ids.is_empty() {
        None
    } else {
        Some(observe_evidence(services, project_id, project_roots, evidence_ids).await)
    };
    for hit in &hits {
        let evidence_freshness = audited_blackboard_freshness(hit, audit.as_ref());
        let premise_freshness = audited_premise_freshness(hit, audit.as_ref());
        let verification = audited_verification(
            hit.entry.value.verification,
            evidence_freshness,
            premise_freshness,
        );
        if !matches!(
            verification,
            BlackboardVerification::SourceVerified | BlackboardVerification::UserConfirmed
        ) {
            return Err(FunctionCallError::RespondToModel(format!(
                "blackboard premise is not currently source-verified or user-confirmed: {}; query or reread current support before recording the derived conclusion",
                hit.entry.id
            )));
        }
    }
    Ok(resolved)
}

pub(super) fn premise_schema() -> serde_json::Value {
    json!({
        "type": "array",
        "maxItems": 16,
        "description": "Exact revisions of current sourceVerified or userConfirmed blackboard entries whose meaning this entry depends on. Premises are semantic provenance, not direct source evidence, and never make this entry sourceVerified. Query the premise immediately before pinning it.",
        "items": {
            "type": "object",
            "properties": {
                "entryId": {"type": "string"},
                "revision": {"type": "integer", "minimum": 1}
            },
            "required": ["entryId", "revision"],
            "additionalProperties": false
        }
    })
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "blackboard_premises_tests.rs"]
mod tests;
