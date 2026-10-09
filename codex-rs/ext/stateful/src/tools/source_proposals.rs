//! The discriminated nonbinding alternative on blackboard_record_batch.
use super::bounded_json_output;
use super::bounded_respond;
use super::preflight_receipts;
use super::receipt_error;
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
    event_sink: Option<&dyn crate::StatefulEventSink>,
    call: &ToolCall<'_>,
    batch: ProposalBatch,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let ProposalBatch {
        kind: ProposalBatchType::SourceProposal,
        records,
    } = batch;
    if records.is_empty() || records.len() > 24 {
        return Err(bounded_respond(
            call,
            "source proposals require 1-24 whole units",
        ));
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
        codex_protocol::ThreadId::from_string(thread)
            .map_err(|error| bounded_respond(call, &error.to_string()))?,
        project,
    )
    .await
    .map_err(|error| bounded_respond(call, &error.to_string()))?
    .ok_or_else(|| {
        bounded_respond(
            call,
            "source proposal refused: authoritative project binding missing",
        )
    })?;
    let node = services
        .project_node_id(project)
        .await
        .map_err(|error| bounded_respond(call, &error))?;
    let store = services
        .blackboard()
        .await
        .map_err(|error| bounded_respond(call, &error.to_string()))?;
    // Preserve only bounded refusal handles and receipt words before the writer consumes
    // the request.
    let mut routes = Vec::with_capacity(records.len());
    let mut shown = Vec::with_capacity(records.len());
    for record in &records {
        routes.push(record.source_id.clone());
        shown.push((record.category, record.interpretation.clone()));
    }
    match store
        .propose_sources(&admission, node, &call.turn_id, records)
        .await
    {
        Ok(results) => {
            if let Some(sink) = event_sink {
                // Every committed proposal of one call shares one group, the receipt's Undo target.
                let first = results.iter().find_map(|result| {
                    result.entry_id.as_deref().and_then(|id| {
                        codex_project_intelligence::BlackboardEntryId::parse(id).ok()
                    })
                });
                let receipt_id = match first {
                    Some(id) => store
                        .proposal_group_id(project, &id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_default(),
                    None => String::new(),
                };
                sink.emit(crate::StatefulEvent::MemoryReceipt(proposal_receipt(
                    project,
                    thread,
                    &call.turn_id,
                    receipt_id,
                    &results,
                    &shown,
                )));
            }
            bounded_json_output(call, envelope(json!(results)))
        }
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

/// The user-facing receipt of committed proposals: what was kept, under which category, and
/// the model's labelled reading (never the user's words, which the exact source keeps).
fn proposal_receipt(
    project: &str,
    thread: &str,
    turn: &str,
    receipt_id: String,
    results: &[codex_project_intelligence::ProposalResult],
    shown: &[(codex_project_intelligence::ProposalCategory, String)],
) -> crate::MemoryReceipt {
    use crate::ReceiptStatus;
    use codex_project_intelligence::KnowledgeCategory;
    use codex_project_intelligence::ProposalCategory;
    let members = results
        .iter()
        .zip(shown)
        .map(|(result, (category, interpretation))| {
            let category = match category {
                ProposalCategory::Rule => KnowledgeCategory::Rule,
                ProposalCategory::Background => KnowledgeCategory::Background,
                ProposalCategory::AttributedContext => KnowledgeCategory::AttributedContext,
                ProposalCategory::Decision => KnowledgeCategory::Decision,
                ProposalCategory::BrainstormOption => KnowledgeCategory::BrainstormOption,
                ProposalCategory::RuledOut => KnowledgeCategory::RuledOut,
                ProposalCategory::OpenCheck => KnowledgeCategory::OpenCheck,
                ProposalCategory::Note => KnowledgeCategory::Note,
            };
            let status = match result.status {
                ProposalStatus::Proposed => ReceiptStatus::Proposed,
                ProposalStatus::Pending => ReceiptStatus::Pending,
                ProposalStatus::Omitted => ReceiptStatus::Omitted,
                ProposalStatus::Refused => ReceiptStatus::Refused,
            };
            crate::ReceiptMember::new(
                result.entry_id.clone().zip(result.revision),
                category,
                status,
                interpretation,
            )
        })
        .collect::<Vec<_>>();
    crate::MemoryReceipt {
        project_id: project.to_string(),
        thread_id: thread.to_string(),
        turn_id: Some(turn.to_string()),
        undoable: !receipt_id.is_empty()
            && members.iter().any(|member| {
                matches!(
                    member.status,
                    ReceiptStatus::Proposed | ReceiptStatus::Pending
                )
            }),
        receipt_id,
        kind: crate::ReceiptKind::Proposals,
        members,
    }
}

/// Fields of one sourceProposal record. They share the batch record item with Agent
/// findings because unions are not portable across tool-call providers; decoding still
/// keeps the two shapes apart by the batch's `type`.
pub(super) fn record_properties() -> serde_json::Value {
    let span = json!({"type":"object","properties":{
        "startByte":{"type":"integer"},"endByte":{"type":"integer"},
        "role":{"type":"string","enum":["body","heading","attribution","qualifier","duration","scope","evidence","temporal"]}
    },"required":["startByte","endByte","role"],"additionalProperties":false});
    json!({
        "sourceId":{"type":"string"},"digest":{"type":"string"},"sourceRevision":{"type":"integer"},"partIndex":{"type":"integer"},
        "spans":{"type":"array","items":span},
        "category":{"type":"string","enum":["rule","background","attributedContext","decision","brainstormOption","ruledOut","openCheck","note"]},
        "interpretation":{"type":"string"},"dependency":{"type":"string","enum":["scope","attribution","referent","promotion"]},
        "eventStatus":{"type":"string","enum":["unknown","sourceReportedOccurrence","currentAssertion","plan","cancelledPlan","uncertainReport","proposedModel"]},
        "attribution":{"type":"string","enum":["unknown","historicalUser","reportedThirdParty","reportedAssistant","importedMaterial"]},"speakerSpan":{"type":"integer"},
        "temporal":{"type":"object","properties":{
            "sourceTimeSpan":{"type":"integer"},"eventTimeSpan":{"type":"integer"},"anchorSpan":{"type":"integer"},
            "form":{"type":"string","enum":["point","interval","duration","recurrence","unresolvedRelative"]}
            ,"precision":{"type":"string","enum":["second","minute","hour","day","month","year","approximate"]},
            "inclusiveStart":{"type":"boolean"},"inclusiveEnd":{"type":"boolean"}
        },"required":["form"],"additionalProperties":false}
    })
}
