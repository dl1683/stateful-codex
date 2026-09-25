use std::collections::HashSet;
use std::fmt::Write;
use std::path::PathBuf;

use codex_extension_api::FunctionCallError;
use codex_project_intelligence::BlackboardEntryId;
use codex_project_intelligence::BlackboardEntryState;
use codex_project_intelligence::BlackboardImportance;
use codex_project_intelligence::BlackboardVerification;
use codex_project_intelligence::RootBlackboardQuery;
use codex_stateful_runtime::ObligationPacket;
use serde_json::Value;
use serde_json::json;

use crate::services::ProjectIntelligenceServices;
use crate::source_freshness::AuditedEvidenceFreshness;
use crate::source_freshness::audited_blackboard_freshness;
use crate::source_freshness::audited_verification;
use crate::source_freshness::observe_evidence;

const MAX_FINAL_CHECKLIST_ITEMS: usize = 16;
const MAX_FINAL_CHECKLIST_ITEM_BYTES: usize = 640;
pub(crate) const MAX_MATERIAL_ROOT_FINDINGS: usize = 8;
pub(crate) const MAX_MATERIAL_HISTORICAL_FINDINGS: usize = 8;

pub(crate) struct HistoricalFindingReference {
    pub(crate) entry_id: String,
    pub(crate) revision: u64,
}

pub(crate) struct CompletionRecord {
    pub(crate) result: String,
    pub(crate) checklist: Vec<Value>,
    pub(crate) omitted_checklist_items: usize,
}

pub(crate) struct CompletionRequest<'a> {
    pub(crate) project_id: &'a str,
    pub(crate) project_roots: &'a [PathBuf],
    pub(crate) result: &'a str,
    pub(crate) packet: &'a ObligationPacket,
    pub(crate) root_revision: u64,
    pub(crate) material_root_findings: &'a [String],
    pub(crate) material_historical_findings: &'a [HistoricalFindingReference],
}

struct ChecklistItem {
    category: &'static str,
    text: String,
}

struct MaterialChecklist {
    items: Vec<ChecklistItem>,
    source_fingerprints: HashSet<String>,
}

pub(crate) async fn prepare_completion(
    services: &ProjectIntelligenceServices,
    request: CompletionRequest<'_>,
) -> Result<CompletionRecord, FunctionCallError> {
    let CompletionRequest {
        project_id,
        project_roots,
        result,
        packet,
        root_revision,
        material_root_findings,
        material_historical_findings,
    } = request;
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

    if material_historical_findings.len() > MAX_MATERIAL_HISTORICAL_FINDINGS {
        return Err(respond(format!(
            "materialHistoricalFindings accepts at most {MAX_MATERIAL_HISTORICAL_FINDINGS} entries"
        )));
    }

    let mut material = material_root_checklist(
        project_id,
        services,
        project_roots,
        root_revision,
        material_root_findings,
    )
    .await?;
    let historical =
        material_historical_checklist(project_id, services, material_historical_findings).await?;
    material.items.extend(historical.items);
    material
        .source_fingerprints
        .extend(historical.source_fingerprints);
    validate_source_fingerprint_references(result, packet, &material.source_fingerprints)?;

    let mut checklist = material.items;
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
            "historicalFinding" => "Historical finding",
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
    project_roots: &[PathBuf],
    expected_root_revision: u64,
    requested_references: &[String],
) -> Result<MaterialChecklist, FunctionCallError> {
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
    let mut selected = Vec::with_capacity(requested_references.len());
    for reference in requested_references {
        let Some(raw_index) = reference.strip_prefix('E') else {
            return Err(invalid_root_alias(reference, expected_root_revision));
        };
        let Ok(index) = raw_index.parse::<usize>() else {
            return Err(invalid_root_alias(reference, expected_root_revision));
        };
        if index == 0 || raw_index.starts_with('0') {
            return Err(invalid_root_alias(reference, expected_root_revision));
        }
        let Some(hit) = projection.data.get(index - 1) else {
            return Err(respond(format!(
                "unknown material root finding alias {reference} at root revision {expected_root_revision}"
            )));
        };
        selected.push((reference, hit));
    }
    let evidence_audit = if selected.is_empty() {
        None
    } else {
        Some(
            observe_evidence(
                services,
                project_id,
                project_roots,
                selected
                    .iter()
                    .flat_map(|(_, hit)| &hit.entry.value.evidence)
                    .map(|evidence| evidence.context_map_entry_id.clone()),
            )
            .await,
        )
    };
    let context_map = services.context_map().await.map_err(respond)?;
    let mut material = MaterialChecklist {
        items: Vec::with_capacity(requested_references.len()),
        source_fingerprints: HashSet::new(),
    };
    for (reference, hit) in selected {
        let evidence_freshness = audited_blackboard_freshness(hit, evidence_audit.as_ref());
        if hit.entry.value.verification == BlackboardVerification::SourceVerified
            && evidence_freshness != AuditedEvidenceFreshness::Current
        {
            return Err(respond(format!(
                "material root finding {reference} evidence is {}; read current evidence and revise or supersede the finding before completing",
                freshness_name(evidence_freshness)
            )));
        }
        let effective_verification =
            audited_verification(hit.entry.value.verification, evidence_freshness);
        let mut sources = Vec::new();
        for evidence in &hit.entry.value.evidence {
            material
                .source_fingerprints
                .insert(evidence.source_fingerprint.as_str().to_ascii_lowercase());
            let Some(route) = context_map
                .get_hit(project_id, &evidence.context_map_entry_id)
                .await
                .map_err(respond)?
            else {
                continue;
            };
            let locator = match evidence.line_range {
                Some(range) => format!(
                    "{}:L{}-L{}",
                    route.source.relative_path, range.start, range.end
                ),
                None => route.source.relative_path.to_string(),
            };
            let source = format!("{locator}@{}", evidence.source_fingerprint);
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        let sources = if sources.is_empty() {
            String::new()
        } else {
            format!(" sources=[{}]", sources.join(","))
        };
        material.items.push(ChecklistItem {
            category: "rootFinding",
            text: bounded_item(&format!(
                "{reference} [{}; verification={}; evidence={}] {}{sources}",
                importance_name(hit.entry.value.importance),
                verification_name(effective_verification),
                freshness_name(evidence_freshness),
                hit.entry.value.content,
            )),
        });
    }
    Ok(material)
}

fn invalid_root_alias(reference: &str, expected_root_revision: u64) -> FunctionCallError {
    respond(format!(
        "invalid material root finding alias {reference}; use aliases such as E1 from root revision {expected_root_revision}"
    ))
}

async fn material_historical_checklist(
    project_id: &str,
    services: &ProjectIntelligenceServices,
    requested_references: &[HistoricalFindingReference],
) -> Result<MaterialChecklist, FunctionCallError> {
    let blackboard = services.blackboard().await.map_err(respond)?;
    let context_map = services.context_map().await.map_err(respond)?;
    let mut unique_references = HashSet::new();
    let mut material = MaterialChecklist {
        items: Vec::with_capacity(requested_references.len()),
        source_fingerprints: HashSet::new(),
    };
    for reference in requested_references {
        if !unique_references.insert(reference.entry_id.as_str()) {
            return Err(respond(format!(
                "materialHistoricalFindings contains duplicate entry {}",
                reference.entry_id
            )));
        }
        let entry_id = BlackboardEntryId::parse(reference.entry_id.clone()).map_err(respond)?;
        let entry = blackboard
            .get_entry(project_id, &entry_id)
            .await
            .map_err(respond)?
            .ok_or_else(|| {
                respond(format!(
                    "unknown historical blackboard entry {}",
                    reference.entry_id
                ))
            })?;
        if entry.revision != reference.revision {
            return Err(respond(format!(
                "historical blackboard entry {} changed from revision {} to {}",
                reference.entry_id, reference.revision, entry.revision
            )));
        }
        if entry.state == BlackboardEntryState::Active {
            return Err(respond(format!(
                "materialHistoricalFindings entry {} is active; select its root alias in materialRootFindings instead",
                reference.entry_id
            )));
        }
        let mut sources = Vec::new();
        for evidence in &entry.value.evidence {
            let fingerprint = evidence.source_fingerprint.as_str().to_string();
            material
                .source_fingerprints
                .insert(fingerprint.to_ascii_lowercase());
            let locator = context_map
                .get_hit(project_id, &evidence.context_map_entry_id)
                .await
                .map_err(respond)?
                .map_or_else(
                    || evidence.context_map_entry_id.to_string(),
                    |route| match evidence.line_range {
                        Some(range) => format!(
                            "{}:L{}-L{}",
                            route.source.relative_path, range.start, range.end
                        ),
                        None => route.source.relative_path.to_string(),
                    },
                );
            sources.push(format!("{locator}@{fingerprint}"));
        }
        let sources = if sources.is_empty() {
            String::new()
        } else {
            format!(" sources=[{}]", sources.join(","))
        };
        material.items.push(ChecklistItem {
            category: "historicalFinding",
            text: bounded_item(&format!(
                "{}@r{} [state={}; verification={}] {}{sources}",
                reference.entry_id,
                reference.revision,
                entry_state_name(entry.state),
                verification_name(entry.value.verification),
                entry.value.content,
            )),
        });
    }
    Ok(material)
}

fn validate_source_fingerprint_references(
    result: &str,
    packet: &ObligationPacket,
    allowed: &HashSet<String>,
) -> Result<(), FunctionCallError> {
    let packet = serde_json::to_string(packet).map_err(respond)?;
    for fingerprint in sha256_references(result).chain(sha256_references(&packet)) {
        if !allowed.contains(&fingerprint.to_ascii_lowercase()) {
            return Err(respond(format!(
                "completion cites unknown source fingerprint {fingerprint}; select its exact entry and revision in materialHistoricalFindings or remove the opaque identifier"
            )));
        }
    }
    Ok(())
}

fn sha256_references(value: &str) -> impl Iterator<Item = &str> {
    value.match_indices("sha256:").filter_map(|(start, _)| {
        let end = start + "sha256:".len() + 64;
        let candidate = value.get(start..end)?;
        let digest = candidate.strip_prefix("sha256:")?;
        if digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            && value
                .as_bytes()
                .get(end)
                .is_none_or(|byte| !byte.is_ascii_hexdigit())
        {
            Some(candidate)
        } else {
            None
        }
    })
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

fn freshness_name(freshness: AuditedEvidenceFreshness) -> &'static str {
    match freshness {
        AuditedEvidenceFreshness::NotApplicable => "notApplicable",
        AuditedEvidenceFreshness::Current => "current",
        AuditedEvidenceFreshness::Stale => "stale",
        AuditedEvidenceFreshness::SourceUnavailable => "sourceUnavailable",
        AuditedEvidenceFreshness::UncheckedThisTurn => "uncheckedThisTurn",
    }
}

fn entry_state_name(state: BlackboardEntryState) -> &'static str {
    match state {
        BlackboardEntryState::Active => "active",
        BlackboardEntryState::Superseded => "superseded",
        BlackboardEntryState::Tombstoned => "tombstoned",
    }
}

fn respond(error: impl std::fmt::Display) -> FunctionCallError {
    FunctionCallError::RespondToModel(error.to_string())
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
