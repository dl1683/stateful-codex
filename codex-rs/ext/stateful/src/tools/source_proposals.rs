//! The discriminated nonbinding alternative on blackboard_record_batch.
use super::bounded_json_output;
use super::preflight_receipts;
use super::receipt_error;
use super::respond;
use crate::services::ProjectIntelligenceServices;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolOutput;
use codex_project_intelligence::ProposalStatus;
use codex_project_intelligence::SourceProposal;
use serde::Deserialize;
use serde_json::json;

#[cfg(test)]
#[path = "source_proposals_tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProposalBatch {
    #[serde(rename = "type")]
    pub(super) kind: ProposalBatchType,
    pub(super) records: Vec<SourceProposal>,
}

#[derive(Deserialize)]
pub(super) enum ProposalBatchType {
    #[serde(rename = "sourceProposal")]
    SourceProposal,
}

pub(super) async fn record(
    services: &ProjectIntelligenceServices,
    project: &str,
    thread: &str,
    call: &ToolCall<'_>,
    batch: ProposalBatch,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let ProposalBatch {
        kind: ProposalBatchType::SourceProposal,
        records,
    } = batch;
    if records.is_empty() || records.len() > 24 {
        return Err(respond("source proposals require 1-24 whole units"));
    }
    let envelope = |results: serde_json::Value| {
        json!({
            "projectId":project,"applied":false,"results":results,
            "use":"stored for explicit review; automatic proposal recall is unavailable; never standing rules or settled decisions",
        })
    };
    preflight_receipts(
        call,
        &envelope(json!(
            records
                .iter()
                .map(|record| json!({
                    "entryId":"x".repeat(73),"revision":u64::MAX,"status":"proposed",
                    "sourceId":record.source_id,"reason":"x".repeat(240),
                }))
                .collect::<Vec<_>>()
        )),
    )?;
    let admission = codex_state::ThreadProjectAdmission::acquire(
        services.sqlite(),
        codex_protocol::ThreadId::from_string(thread).map_err(respond)?,
        project,
    )
    .await
    .map_err(respond)?
    .ok_or_else(|| respond("source proposal refused: authoritative project binding missing"))?;
    let node = services.project_node_id(project).await.map_err(respond)?;
    let store = services.blackboard().await.map_err(respond)?;
    // Preserve only bounded refusal handles before the writer consumes the request.
    let mut routes = Vec::with_capacity(records.len());
    for record in &records {
        routes.push(record.source_id.clone());
    }
    match store
        .propose_sources(&admission, node, &call.turn_id, records)
        .await
    {
        Ok(results) => bounded_json_output(call, envelope(json!(results))),
        Err(error) => bounded_json_output(
            call,
            envelope(json!(
                routes
                    .into_iter()
                    .map(|source| json!({
                        "entryId":null,"revision":null,"status":ProposalStatus::Refused,
                        "sourceId":source,"reason":receipt_error(&error),
                    }))
                    .collect::<Vec<_>>()
            )),
        ),
    }
}

pub(super) fn schema(agent: serde_json::Value) -> serde_json::Value {
    let span = json!({"type":"object","properties":{
        "startByte":{"type":"integer"},"endByte":{"type":"integer"},
        "role":{"type":"string","enum":["body","heading","attribution","qualifier","duration","scope","evidence","temporal"]}
    },"required":["startByte","endByte","role"],"additionalProperties":false});
    json!({"anyOf":[agent,{"type":"object","properties":{
        "type":{"type":"string","enum":["sourceProposal"]},
        "records":{"type":"array","minItems":1,"maxItems":24,"items":{"type":"object","properties":{
            "sourceId":{"type":"string"},"digest":{"type":"string"},"sourceRevision":{"type":"integer"},"partIndex":{"type":"integer"},
            "spans":{"type":"array","maxItems":8,"items":span},
            "category":{"type":"string","enum":["rule","background","attributedContext","decision","brainstormOption","openCheck","note"]},
            "interpretation":{"type":"string"},"dependency":{"type":"string","enum":["scope","attribution","referent","promotion"]},
            "eventStatus":{"type":"string","enum":["unknown","sourceReportedOccurrence","currentAssertion","plan","cancelledPlan","uncertainReport","proposedModel"]},
            "attribution":{"type":"string","enum":["unknown","historicalUser","reportedThirdParty","reportedAssistant","importedMaterial"]},"speakerSpan":{"type":"integer"},
            "temporal":{"type":"object","properties":{
                "sourceTimeSpan":{"type":"integer"},"eventTimeSpan":{"type":"integer"},"anchorSpan":{"type":"integer"},
                "form":{"type":"string","enum":["point","interval","duration","recurrence","unresolvedRelative"]}
                ,"precision":{"type":"string","enum":["second","minute","hour","day","month","year","approximate"]},
                "inclusiveStart":{"type":"boolean"},"inclusiveEnd":{"type":"boolean"}
            },"required":["form"],"additionalProperties":false}
        },"required":["sourceId","digest","sourceRevision","partIndex","spans","category","interpretation"],"additionalProperties":false}}
    },"required":["type","records"],"additionalProperties":false}]})
}
