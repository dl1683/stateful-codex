use std::collections::HashSet;
use std::fmt::Write;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEvidenceFreshness;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::RootBlackboardQuery;
use codex_stateful_runtime::ObligationPacket;
use serde_json::Value;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;

const MAX_FINAL_CHECKLIST_ITEMS: usize = 16;
const MAX_FINAL_CHECKLIST_ITEM_BYTES: usize = 640;
pub(crate) const MAX_MATERIAL_ROOT_FINDINGS: usize = 8;

pub(crate) struct CompletionRecord {
    pub(crate) result: String,
    pub(crate) checklist: Vec<Value>,
    pub(crate) omitted_checklist_items: usize,
}

struct ChecklistItem {
    category: &'static str,
    text: String,
}

pub(crate) async fn prepare_completion(
    project_id: &str,
    services: &ProjectIntelligenceServices,
    result: &str,
    packet: &ObligationPacket,
    root_revision: u64,
    material_root_findings: &[String],
) -> Result<CompletionRecord, FunctionCallError> {
    if material_root_findings.len() > MAX_MATERIAL_ROOT_FINDINGS {
        return Err(respond(format!(
            "materialRootFindings accepts at most {MAX_MATERIAL_ROOT_FINDINGS} root aliases; select the highest-priority findings directly material to the outcome and preserve the remainder in the final semantic obligation"
        )));
    }
    let mut unique_references = HashSet::new();
    if let Some(reference) = material_root_findings
        .iter()
        .find(|reference| !unique_references.insert(reference.as_str()))
    {
        return Err(respond(format!(
            "materialRootFindings contains duplicate reference {reference}"
        )));
    }

    let mut checklist =
        material_root_checklist(project_id, services, root_revision, material_root_findings)
            .await?;
    checklist.extend(packet_checklist(packet));
    if checklist.is_empty() {
        return Err(respond(
            "completed requires a final semantic obligation with at least one learning, implication, uncertainty, or blocker",
        ));
    }
    let omitted_checklist_items = checklist.len().saturating_sub(MAX_FINAL_CHECKLIST_ITEMS);
    checklist.truncate(MAX_FINAL_CHECKLIST_ITEMS);

    let mut durable_result = result.to_string();
    durable_result.push_str("\n\nDurable completion basis:");
    for item in &checklist {
        let label = match item.category {
            "rootFinding" => "Root finding",
            "learning" => "Learning",
            "implication" => "Implication",
            "uncertainty" => "Uncertainty",
            "blocker" => "Blocker",
            _ => unreachable!("completion checklist categories are static"),
        };
        let _ = write!(durable_result, "\n- {label}: {}", item.text);
    }
    if omitted_checklist_items > 0 {
        let _ = write!(
            durable_result,
            "\n- {omitted_checklist_items} additional final-obligation items remain in the structured obligation record."
        );
    }

    Ok(CompletionRecord {
        result: durable_result,
        checklist: checklist
            .into_iter()
            .map(|item| json!({"category": item.category, "text": item.text}))
            .collect(),
        omitted_checklist_items,
    })
}

async fn material_root_checklist(
    project_id: &str,
    services: &ProjectIntelligenceServices,
    expected_root_revision: u64,
    requested_references: &[String],
) -> Result<Vec<ChecklistItem>, FunctionCallError> {
    let projection = services
        .blackboard()
        .await
        .map_err(respond)?
        .root_projection(RootBlackboardQuery {
            project_id: project_id.to_string(),
            max_entries: 256,
        })
        .await
        .map_err(respond)?;
    if projection.revision != expected_root_revision {
        return Err(respond(format!(
            "root blackboard changed from revision {expected_root_revision} to {}; review the current root aliases before completing",
            projection.revision
        )));
    }
    let context_map = services.context_map().await.map_err(respond)?;
    let mut items = Vec::with_capacity(requested_references.len());
    for reference in requested_references {
        let Some(raw_index) = reference.strip_prefix('E') else {
            return Err(respond(format!(
                "invalid material root finding alias {reference}; use aliases such as E1 from root revision {expected_root_revision}"
            )));
        };
        let Ok(index) = raw_index.parse::<usize>() else {
            return Err(respond(format!(
                "invalid material root finding alias {reference}; use aliases such as E1 from root revision {expected_root_revision}"
            )));
        };
        if index == 0 || raw_index.starts_with('0') {
            return Err(respond(format!(
                "invalid material root finding alias {reference}; use aliases such as E1 from root revision {expected_root_revision}"
            )));
        }
        let Some(hit) = projection.data.get(index - 1) else {
            return Err(respond(format!(
                "unknown material root finding alias {reference} at root revision {expected_root_revision}"
            )));
        };
        let mut sources = Vec::new();
        for evidence in &hit.entry.value.evidence {
            let Some(route) = context_map
                .get_hit(project_id, &evidence.context_map_entry_id)
                .await
                .map_err(respond)?
            else {
                continue;
            };
            let source = match evidence.line_range {
                Some(range) => format!(
                    "{}:L{}-L{}",
                    route.source.relative_path, range.start, range.end
                ),
                None => route.source.relative_path.to_string(),
            };
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        let sources = if sources.is_empty() {
            String::new()
        } else {
            format!(" sources=[{}]", sources.join(","))
        };
        items.push(ChecklistItem {
            category: "rootFinding",
            text: bounded_item(&format!(
                "{reference} [{}; verification={}; evidence={}] {}{sources}",
                importance_name(hit.entry.value.importance),
                verification_name(hit.effective_verification),
                freshness_name(hit.evidence_freshness),
                hit.entry.value.content,
            )),
        });
    }
    Ok(items)
}

fn packet_checklist(packet: &ObligationPacket) -> Vec<ChecklistItem> {
    [
        ("learning", &packet.learning),
        ("implication", &packet.implication),
        ("uncertainty", &packet.uncertainty),
        ("blocker", &packet.blockers),
    ]
    .into_iter()
    .flat_map(|(category, items)| {
        items.iter().map(move |item| ChecklistItem {
            category,
            text: bounded_item(item),
        })
    })
    .collect()
}

fn bounded_item(item: &str) -> String {
    if item.len() <= MAX_FINAL_CHECKLIST_ITEM_BYTES {
        return item.to_string();
    }
    let marker = "…";
    let mut boundary = MAX_FINAL_CHECKLIST_ITEM_BYTES.saturating_sub(marker.len());
    while !item.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{marker}", &item[..boundary])
}

fn importance_name(importance: BlackboardImportance) -> &'static str {
    match importance {
        BlackboardImportance::Critical => "critical",
        BlackboardImportance::High => "high",
        BlackboardImportance::Normal => "normal",
        BlackboardImportance::Low => "low",
    }
}

fn verification_name(verification: BlackboardVerification) -> &'static str {
    match verification {
        BlackboardVerification::Unverified => "unverified",
        BlackboardVerification::SourceVerified => "sourceVerified",
        BlackboardVerification::UserConfirmed => "userConfirmed",
        BlackboardVerification::Disputed => "disputed",
        BlackboardVerification::Stale => "stale",
    }
}

fn freshness_name(freshness: BlackboardEvidenceFreshness) -> &'static str {
    match freshness {
        BlackboardEvidenceFreshness::NotApplicable => "notApplicable",
        BlackboardEvidenceFreshness::Current => "current",
        BlackboardEvidenceFreshness::Stale => "stale",
        BlackboardEvidenceFreshness::SourceUnavailable => "sourceUnavailable",
    }
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}
